use std::net::{SocketAddr, TcpStream, ToSocketAddrs};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

#[cfg(windows)]
use std::os::windows::process::CommandExt;

use super::launch::split_host_port;
use super::*;

/// GET http://127.0.0.1:<dash>/api/status with a short timeout. Raw HTTP/1.0 —
/// no extra deps; the dashboard only listens on loopback.
fn dashboard_status() -> Option<DashboardStatus> {
    use std::io::{Read, Write};
    let addr = std::net::SocketAddr::from(([127, 0, 0, 1], dashboard_port()));
    let mut s = TcpStream::connect_timeout(&addr, Duration::from_millis(700)).ok()?;
    s.set_read_timeout(Some(Duration::from_millis(700))).ok()?;
    s.set_write_timeout(Some(Duration::from_millis(700))).ok()?;
    write!(s, "GET /api/status HTTP/1.0\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n").ok()?;
    let mut buf = Vec::with_capacity(16384);
    let mut tmp = [0u8; 8192];
    loop {
        match s.read(&mut tmp) {
            Ok(0) | Err(_) => break,
            Ok(n) => {
                buf.extend_from_slice(&tmp[..n]);
                if buf.len() > (1 << 20) {
                    break;
                }
            }
        }
    }
    let text = String::from_utf8_lossy(&buf);
    let body = text.split("\r\n\r\n").nth(1).unwrap_or("");
    let v: serde_json::Value = serde_json::from_str(body).ok()?;
    let ports = v.get("ports")?;
    let num = |k: &str, d: u64| v.get(k).and_then(|x| x.as_u64()).unwrap_or(d) as u16;
    Some(DashboardStatus {
        login: ports.get("login").and_then(|x| x.as_bool()).unwrap_or(false),
        game: ports.get("game").and_then(|x| x.as_bool()).unwrap_or(false),
        ip: v.get("ip").and_then(|x| x.as_str()).unwrap_or_default().to_string(),
        login_port: num("login_port", 15001),
        game_port: num("game_port", 10200),
    })
}

/// Cross-check a failed probe against Ultimiadash for addresses pointing at
/// this machine — the dashboard probes the real bind IP, so it reports ONLINE
/// even when the server isn't listening on loopback. Explicit ports that
/// aren't the server's own keep the plain TCP verdict.
fn sync_with_dashboard(mut check: ServerCheck, host: &str, explicit_port: Option<u16>) -> ServerCheck {
    if check.reachable {
        return check;
    }
    let Some(dash) = dashboard_status() else {
        return check;
    };
    let h = host
        .trim()
        .trim_matches(|c| c == '[' || c == ']')
        .trim_end_matches('.')
        .to_lowercase();
    let local = h == "localhost"
        || h == "::1"
        || h.starts_with("127.")
        || (!dash.ip.is_empty() && h == dash.ip.to_lowercase());
    if !local {
        return check;
    }
    if let Some(p) = explicit_port {
        if p != dash.login_port && p != dash.game_port {
            return check;
        }
    }
    if dash.login {
        check.reachable = true;
        check.port = Some(dash.login_port);
        check.error = None;
    } else {
        let prev = check.error.take().unwrap_or_else(|| "unreachable".into());
        check.error = Some(format!("{prev} — ultimiadash reports the local server offline"));
    }
    check.via_dashboard = true;
    check
}

const CHECK_TIMEOUT: Duration = Duration::from_millis(2500);
// Ports probed when the address carries none: the server's login default first,
// then the port SoulLauncher/direct-launch falls back to.
const CHECK_DEFAULT_PORTS: [u16; 2] = [15001, 10000];

fn probe_addr(addr: String) -> Result<u64, String> {
    let sock = addr
        .to_socket_addrs()
        .map_err(|e| format!("cannot resolve address: {e}"))?
        .next()
        .ok_or_else(|| "cannot resolve address".to_string())?;
    let start = Instant::now();
    TcpStream::connect_timeout(&sock, CHECK_TIMEOUT)
        .map_err(|e| e.to_string())?;
    Ok(start.elapsed().as_millis() as u64)
}

/// TCP-connect probe for "host[:port]" game addresses, used by the join
/// page's CHECK SERVER button. A bare host is tried on the usual private
/// server ports so users do not have to guess which one is listening.
pub fn check_server(input: &str) -> ServerCheck {
    let s = match validate_server_address(input) {
        Ok(s) => s,
        Err(e) => {
            return ServerCheck { reachable: false, latency_ms: None, port: None, error: Some(e), via_dashboard: false };
        }
    };
    let has_port = if s.starts_with('[') { s.contains("]:") } else { s.contains(':') };
    let Some((host, port_str)) = split_host_port(&s) else {
        return ServerCheck {
            reachable: false,
            latency_ms: None,
            port: None,
            error: Some("Cannot parse server address.".into()),
            via_dashboard: false,
        };
    };
    let ports: Vec<u16> = if has_port {
        match port_str.parse::<u16>() {
            Ok(p) => vec![p],
            Err(_) => {
                return ServerCheck {
                    reachable: false,
                    latency_ms: None,
                    port: None,
                    error: Some("Invalid port number.".into()),
                    via_dashboard: false,
                }
            }
        }
    } else {
        CHECK_DEFAULT_PORTS.to_vec()
    };
    let explicit_port = if has_port { ports.first().copied() } else { None };
    let mut last_err = String::new();
    let mut check = ServerCheck {
        reachable: false,
        latency_ms: None,
        port: None,
        error: None,
        via_dashboard: false,
    };
    for port in ports {
        let addr = format!("{host}:{port}");
        match probe_addr(addr) {
            Ok(ms) => {
                check.reachable = true;
                check.latency_ms = Some(ms);
                check.port = Some(port);
                break;
            }
            Err(e) => last_err = e,
        }
    }
    if !check.reachable {
        check.error = Some(if last_err.is_empty() { "unreachable".into() } else { last_err });
    }
    sync_with_dashboard(check, &host, explicit_port)
}

/// First non-empty env var among `names` (new name first, legacy fallback).
fn env_path(names: &[&str]) -> Option<PathBuf> {
    names.iter().find_map(|n| {
        std::env::var(n).ok().map(PathBuf::from).filter(|p| !p.as_os_str().is_empty())
    })
}

/// Server run dir: explicit `dir` (Ultimia/server repo root or its run dir),
/// ULTIMIA_RUN/SWEMU_RUN env, the Ultimia workspace layout found by walking up
/// from this exe (<ws>/server/run), then ./run.
fn server_run_dir(dir: Option<&str>) -> PathBuf {
    if let Some(d) = dir.map(str::trim).filter(|s| !s.is_empty()) {
        let p = PathBuf::from(d);
        return if p.join("run").is_dir() { p.join("run") } else { p };
    }
    if let Some(p) = env_path(&["ULTIMIA_RUN", "SWEMU_RUN"]) {
        return p;
    }
    if let Ok(exe) = std::env::current_exe() {
        for anc in exe.ancestors() {
            let ultimia = anc.join("server").join("run");
            if ultimia.is_dir() {
                return ultimia;
            }
            // Public swemu checkout layout: <repo>/run beside <repo>/server.
            if anc.join("run").is_dir() && anc.join("server").join("Cargo.toml").is_file() {
                return anc.join("run");
            }
        }
    }
    PathBuf::from("run")
}

const SERVER_EXE_NAMES: [&str; 2] = ["ultimia-server.exe", "swemu.exe"];
const DASH_EXE_NAMES: [&str; 2] = ["ultimiadash.exe", "swemu-dashboard.exe"];

/// ultimia-server.exe: ULTIMIA_EXE/SWEMU_EXE env, then conventional layouts
/// under `dir` (branded name first, public swemu.exe as fallback).
fn server_exe(dir: Option<&str>) -> PathBuf {
    if let Some(p) = env_path(&["ULTIMIA_EXE", "SWEMU_EXE"]) {
        return p;
    }
    let mut cands: Vec<PathBuf> = Vec::new();
    if let Some(d) = dir.map(str::trim).filter(|s| !s.is_empty()) {
        let p = PathBuf::from(d);
        for name in SERVER_EXE_NAMES {
            cands.push(p.join("server").join("target").join("release").join(name));
            cands.push(p.join(name));
        }
    }
    let run = server_run_dir(dir);
    for name in SERVER_EXE_NAMES {
        cands.push(run.join("..").join("server").join("target").join("release").join(name));
    }
    cands.into_iter().find(|p| p.is_file()).unwrap_or_else(|| PathBuf::from("ultimia-server.exe"))
}

fn server_cfg(dir: Option<&str>) -> serde_json::Value {
    let run = server_run_dir(dir);
    for name in ["ultimia.json", "swemu.json"] {
        if let Ok(s) = std::fs::read_to_string(run.join(name)) {
            if let Ok(v) = serde_json::from_str(&s) {
                return v;
            }
        }
    }
    serde_json::json!({})
}

fn server_login_port(dir: Option<&str>) -> u16 {
    server_cfg(dir).get("login_port").and_then(|v| v.as_u64()).map(|v| v as u16).unwrap_or(15001)
}

/// Default-route LAN address (UDP connect trick — no packet is sent).
fn lan_ip() -> Option<std::net::IpAddr> {
    let sock = std::net::UdpSocket::bind("0.0.0.0:0").ok()?;
    sock.connect("192.0.2.1:80").ok()?;
    sock.local_addr().ok().map(|a| a.ip())
}

/// IP the client should dial for the local server: explicit public_ip/bind_ip
/// from ultimia.json, else the LAN address, else localhost.
fn server_host(dir: Option<&str>) -> std::net::IpAddr {
    let conf = server_cfg(dir);
    for key in ["public_ip", "bind_ip"] {
        if let Some(v) = conf.get(key).and_then(|v| v.as_str()) {
            if v != "auto" {
                if let Ok(ip) = v.parse::<std::net::IpAddr>() {
                    return ip;
                }
            }
        }
    }
    lan_ip().unwrap_or(std::net::IpAddr::V4(std::net::Ipv4Addr::LOCALHOST))
}

fn server_port_open(dir: Option<&str>) -> bool {
    let port = server_login_port(dir);
    let mut ips = vec![std::net::IpAddr::V4(std::net::Ipv4Addr::LOCALHOST), server_host(dir)];
    ips.dedup();
    ips.iter().any(|ip| {
        TcpStream::connect_timeout(&SocketAddr::new(*ip, port), Duration::from_millis(400)).is_ok()
    })
}

/// "ip:port" the game client should connect to for the local emulator.
pub fn emulator_address(dir: Option<&str>) -> String {
    format!("{}:{}", server_host(dir), server_login_port(dir))
}

/// Spawn a detached server/dashboard process, then poll until its port opens
/// (40 × 250ms ≈ 10s). Shared by `emulator_ensure` and `dashboard_ensure`.
fn spawn_and_watch(
    cmd: &mut Command,
    port_open: impl Fn() -> bool,
    started_note: &str,
    timeout_note: &str,
) -> EmulatorStatus {
    match cmd.spawn() {
        Ok(_) => {
            for _ in 0..40 {
                std::thread::sleep(Duration::from_millis(250));
                if port_open() {
                    return EmulatorStatus { running: true, spawned: true, note: started_note.into() };
                }
            }
            EmulatorStatus { running: false, spawned: true, note: timeout_note.into() }
        }
        Err(e) => EmulatorStatus { running: false, spawned: false, note: format!("spawn failed: {e}") },
    }
}

/// Ensure the local server emulator is listening before the client launches.
/// Spawns ultimia-server.exe detached when it is down.
pub fn emulator_ensure(dir: Option<&str>) -> EmulatorStatus {
    if server_port_open(dir) {
        return EmulatorStatus { running: true, spawned: false, note: "server already running".into() };
    }
    let exe_path = server_exe(dir);
    if !exe_path.is_file() {
        return EmulatorStatus { running: false, spawned: false, note: format!("ultimia-server.exe not found: {} — set the emulator folder in Settings → Installation", exe_path.display()) };
    }
    let run_dir = server_run_dir(dir);
    let mut cmd = Command::new(&exe_path);
    cmd.current_dir(&run_dir)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(windows)]
    cmd.creation_flags(CREATE_NO_WINDOW);
    spawn_and_watch(
        &mut cmd,
        || server_port_open(dir),
        "server started",
        "server spawned but the login port never opened — check run/log",
    )
}

/// Dashboard port: DASH_PORT env (same override the dashboard itself honours), else 8377.
pub fn dashboard_port() -> u16 {
    std::env::var("DASH_PORT")
        .ok()
        .and_then(|v| v.parse::<u16>().ok())
        .unwrap_or(8377)
}

/// ultimatelydash.exe: ULTIMIA_DASHBOARD_EXE/SWEMU_DASHBOARD_EXE env, then the
/// sibling checkout next to the server repo (<ws>/dashboard or the legacy
/// <ws>/swemu-dashboard layout).
fn dashboard_exe(dir: Option<&str>) -> PathBuf {
    if let Some(p) = env_path(&["ULTIMIA_DASHBOARD_EXE", "SWEMU_DASHBOARD_EXE"]) {
        return p;
    }
    let mut cands: Vec<PathBuf> = Vec::new();
    // Exe-relative workspace walk: <ws>/dashboard/target/release/<exe>.
    if let Ok(exe) = std::env::current_exe() {
        for anc in exe.ancestors() {
            for name in DASH_EXE_NAMES {
                cands.push(anc.join("dashboard").join("target").join("release").join(name));
                cands.push(anc.join("swemu-dashboard").join("target").join("release").join(name));
            }
        }
    }
    // run dir → repo → workspace sibling (both layouts).
    let from_run = |run: &Path, cands: &mut Vec<PathBuf>| {
        for name in DASH_EXE_NAMES {
            cands.push(run.join("..").join("..").join("dashboard").join("target").join("release").join(name));
            cands.push(run.join("..").join("..").join("swemu-dashboard").join("target").join("release").join(name));
        }
    };
    if let Some(d) = dir.map(str::trim).filter(|s| !s.is_empty()) {
        let p = PathBuf::from(d);
        for name in DASH_EXE_NAMES {
            // `dir` given as a repo root: <repo>/../dashboard|swemu-dashboard.
            cands.push(p.join("..").join("dashboard").join("target").join("release").join(name));
            cands.push(p.join("..").join("swemu-dashboard").join("target").join("release").join(name));
        }
        from_run(&server_run_dir(Some(d)), &mut cands);
    }
    if let Some(run) = env_path(&["ULTIMIA_RUN", "SWEMU_RUN"]) {
        from_run(&run, &mut cands);
    }
    cands.into_iter().find(|p| p.is_file()).unwrap_or_else(|| PathBuf::from("ultimiadash.exe"))
}

fn dashboard_port_open() -> bool {
    TcpStream::connect_timeout(
        &SocketAddr::from(([127, 0, 0, 1], dashboard_port())),
        Duration::from_millis(400),
    )
    .is_ok()
}

/// Launch the standalone dashboard app. Its single-instance plugin focuses an
/// already-open window; a fresh instance opens its own. When the exe is missing
/// but something already serves the port (e.g. a headless dashboard), report
/// running without spawning so the caller can fall back to an embedded webview.
pub fn dashboard_ensure(dir: Option<&str>) -> EmulatorStatus {
    let exe_path = dashboard_exe(dir);
    if !exe_path.is_file() {
        if dashboard_port_open() {
            return EmulatorStatus { running: true, spawned: false, note: "dashboard server running (app exe not found)".into() };
        }
        return EmulatorStatus { running: false, spawned: false, note: format!("ultimiadash.exe not found: {} — set ULTIMIA_DASHBOARD_EXE", exe_path.display()) };
    }
    let run_dir = server_run_dir(dir);
    let mut cmd = Command::new(&exe_path);
    cmd.current_dir(&run_dir)
        // New names first; legacy SWEMU_* kept so an older dashboard exe
        // found through the fallback paths still gets its overrides.
        .env("ULTIMIA_RUN", &run_dir)
        .env("ULTIMIA_EXE", server_exe(dir))
        .env("SWEMU_RUN", &run_dir)
        .env("SWEMU_EXE", server_exe(dir))
        .env("DASH_PORT", dashboard_port().to_string())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(windows)]
    cmd.creation_flags(CREATE_NO_WINDOW);
    spawn_and_watch(
        &mut cmd,
        dashboard_port_open,
        "dashboard opened",
        "dashboard spawned but the port never opened",
    )
}
