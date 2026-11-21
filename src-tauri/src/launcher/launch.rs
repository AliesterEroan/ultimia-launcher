use std::path::Path;
use std::process::{Command, Stdio};

#[cfg(windows)]
use std::os::windows::process::CommandExt;

use super::detect::{steam_display_name, steam_game_dir};
use super::*;

const LAUNCHER_MISSING: &str =
    "SoulLauncher.exe not found next to UltimiaLauncher.exe or in your SoulWorker folder.";

/// Host or [IPv6], optional :port. Must not start with '-' (SoulLauncher's
/// --server value is optional, so a dash-leading value would be parsed as
/// another flag) and may not carry whitespace or shell metacharacters.
pub fn validate_server_address(input: &str) -> Result<String, String> {
    static RE: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    let s = input.trim();
    if s.is_empty() {
        return Err("Server address is empty.".into());
    }
    if s.starts_with('-') {
        return Err("Server address must not start with '-'.".into());
    }
    let re = RE.get_or_init(|| {
        regex::Regex::new(
            r"^(\[[0-9A-Fa-f:]+\]|[A-Za-z0-9](?:[A-Za-z0-9.\-]*[A-Za-z0-9])?)(?::(\d{1,5}))?$",
        )
        .expect("static regex")
    });
    let caps = re
        .captures(s)
        .ok_or_else(|| "Invalid address. Use host:port, e.g. 192.168.0.10:15001.".to_string())?;
    if let Some(port) = caps.get(2) {
        let n: u32 = port.as_str().parse().map_err(|_| "Invalid port number.")?;
        if n == 0 || n > 65535 {
            return Err("Port must be between 1 and 65535.".into());
        }
    }
    Ok(s.to_string())
}

const GLOBAL_PENDING: &str = "global-pending: Global sign-in through VFUN is not finished yet.";

fn is_global(server: &str) -> bool {
    server.is_empty() || server == "global"
}

pub fn build_args(config: &LaunchConfig, exe: &Path) -> Result<Vec<String>, String> {
    let mut args = vec![exe.to_string_lossy().into_owned()];
    let server = config.server.trim();
    // SoulLauncher is the private-server launcher; Global must never be routed through it.
    if is_global(server) {
        return Err(GLOBAL_PENDING.into());
    }
    args.push("--server".into());
    args.push(validate_server_address(server).map_err(|e| format!("invalid-server: {e}"))?);
    match config.install_source.as_str() {
        "vfun" => args.push("--vfun-install".into()),
        "custom" => {
            let folder = config
                .custom_folder
                .as_deref()
                .map(str::trim)
                .filter(|f| !f.is_empty())
                .ok_or_else(|| "game-not-found: Choose a game folder first.".to_string())?;
            if !Path::new(folder).is_dir() {
                return Err(format!("game-not-found: Folder does not exist: {folder}"));
            }
            args.push("--game".into());
            args.push(folder.to_string());
        }
        "steam" => {}
        other => return Err(format!("Unknown install source: {other}")),
    }
    if config.skip_version_check {
        args.push("--skip-version-check".into());
    }
    // Ultimia is the UI: SoulLauncher's own message boxes must never appear.
    args.push("--silent".into());
    Ok(args)
}

/// Fire-and-forget spawn: no pipes to drain, no exit wait, no kill. Used when
/// the launched process owns the game session and must outlive us.
fn spawn_detached(exe: &Path, args: &[String]) -> Result<(), String> {
    let mut cmd = Command::new(exe);
    cmd.args(args)
        .current_dir(exe.parent().unwrap_or(Path::new(".")))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(windows)]
    cmd.creation_flags(CREATE_NO_WINDOW);
    cmd.spawn()
        .map(|_| ())
        .map_err(|e| format!("launch-failed: Could not start SoulLauncher: {e}"))
}

fn account_key() -> Option<String> {
    let appdata = std::env::var("APPDATA").ok()?;
    let path = Path::new(&appdata)
        .join("SoulLauncher")
        .join("server_token.txt");
    std::fs::read_to_string(path)
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// Split "host:port" for the direct launch. SoulLauncher defaults a missing
/// port to 10000; we do the same so portless custom addresses still work.
pub(super) fn split_host_port(server: &str) -> Option<(String, String)> {
    let s = server.trim();
    if let Some(rest) = s.strip_prefix('[') {
        // Bracketed IPv6: only [::1]:port is usable; anything else falls back.
        let (host, rest) = rest.split_once(']')?;
        let port = rest.strip_prefix(':')?;
        if port.is_empty() {
            return None;
        }
        return Some((format!("[{host}]"), port.to_string()));
    }
    if let Some((host, port)) = s.rsplit_once(':') {
        if !host.is_empty() && !port.is_empty() {
            return Some((host.to_string(), port.to_string()));
        }
    }
    Some((s.to_string(), "10000".to_string()))
}

/// Hand-off arg the client parses to open the ID/password panel (LOGIN_REQ
/// 2/0x01) — the flow verified against client 2.3.34.7. The `fromVLauncher::`
/// token hand-off leaves the server-select scene empty on that build, so the
/// emulator uses this arg instead. We never write the game's General.ini; the
/// client uses whatever server address its own config already holds.
const EMULATOR_HANDOFF: &str = "VFunQA:: MultiExecuteClient";

fn emulator_args(exe: &Path) -> Vec<String> {
    vec![exe.to_string_lossy().into_owned(), EMULATOR_HANDOFF.into()]
}

/// Local-emulator launch: `SoulWorker.exe "VFunQA:: MultiExecuteClient"`.
fn emulator_arg_launch(config: &LaunchConfig) -> Result<LaunchOutcome, String> {
    let exe = crate::vfun::game_exe(config)?;
    let dir = exe.parent().unwrap_or(Path::new(".")).to_path_buf();
    crate::vfun::shell_execute_open(&exe, EMULATOR_HANDOFF, &dir)?;
    Ok(LaunchOutcome {
        args: emulator_args(&exe),
        exit_code: Some(0),
        stdout: String::new(),
        stderr: String::new(),
        timed_out: false,
    })
}

/// Direct SoulWorker.exe launch for private servers — the same hand-off
/// SoulLauncher.exe produces (`IP:x PORT:y fromVLauncher::swserver:...`),
/// minus the launcher process and its dialogs. Returns Ok(None) when we lack
/// what the direct path needs (no account key, bad address, game not found)
/// so the caller can fall back to SoulLauncher.exe.
fn private_direct_launch(config: &LaunchConfig) -> Result<Option<LaunchOutcome>, String> {
    let Some(key) = account_key() else {
        return Ok(None);
    };
    let Some((host, port)) = split_host_port(config.server.trim()) else {
        return Ok(None);
    };
    let exe = match crate::vfun::game_exe(config) {
        Ok(exe) => exe,
        Err(_) => return Ok(None),
    };
    let dir = exe.parent().unwrap_or(Path::new("."));
    let handoff = format!("IP:{host} PORT:{port} fromVLauncher::swserver:{key}:English:GBL");
    crate::vfun::shell_execute_open(&exe, &handoff, dir)?;
    Ok(Some(LaunchOutcome {
        // The hand-off carries the account key — keep it out of the UI/log.
        args: vec![exe.to_string_lossy().into_owned()],
        exit_code: Some(0),
        stdout: String::new(),
        stderr: String::new(),
        timed_out: false,
    }))
}

/// Steam's SoulWorker.exe takes its whole server list on the command line —
/// `--ip GBL <ip> --ip GBL <ip> --ip CHN <ip> --port <port>` (captured from
/// Steam's own -applaunch args). No hand-off token is needed: steam_api64
/// supplies the session identity, so this works for Global, private servers
/// and the local emulator alike — `--ip` accepts any address.
fn steam_direct_launch(config: &LaunchConfig) -> Result<Option<LaunchOutcome>, String> {
    // Only the detected Steam install takes the --ip/--port arg style;
    // 'custom' folders may hold the VFUN client, which uses hand-offs.
    if config.install_source != "steam" {
        return Ok(None);
    }
    let dir = steam_game_dir().ok_or_else(|| "game-not-found: Steam Soulworker install not found.".to_string())?;
    let exe = dir.join("SoulWorker.exe");
    if !exe.is_file() {
        return Err(format!("game-not-found: SoulWorker.exe is not in {}.", dir.display()));
    }

    // (region tag, ip) triples — Global uses the three official slots,
    // everything else points all regions at the one address.
    let mut ips: Vec<(&'static str, String)> = Vec::new();
    let mut port = "10000".to_string();
    let server = config.server.trim();
    if is_global(server) {
        // ULTIMIA_GBL_RELAY=host:port reroutes every slot through a local
        // relay (the sniffer) — same trick as the VFUN hand-off path.
        if let Ok(relay) = std::env::var("ULTIMIA_GBL_RELAY") {
            let relay = relay.trim().to_string();
            if !relay.is_empty() {
                if let Some((h, p)) = split_host_port(&relay) {
                    port = p;
                    ips = vec![("GBL", h.clone()), ("GBL", h.clone()), ("CHN", h)];
                }
            }
        }
        if ips.is_empty() {
            for (tag, addr) in [
                ("GBL", &config.official_servers.global1),
                ("GBL", &config.official_servers.global2),
                ("CHN", &config.official_servers.china),
            ] {
                if let Some((h, p)) = split_host_port(addr) {
                    if port == "10000" {
                        port = p;
                    }
                    ips.push((tag, h));
                }
            }
        }
    } else {
        let Some((h, p)) = split_host_port(server) else {
            return Ok(None);
        };
        port = p;
        ips = vec![("GBL", h.clone()), ("GBL", h.clone()), ("CHN", h)];
    }
    if ips.is_empty() {
        return Ok(None);
    }

    let mut args: Vec<String> = vec![exe.to_string_lossy().into_owned()];
    for (tag, ip) in &ips {
        args.push("--ip".into());
        args.push((*tag).to_string());
        args.push(ip.clone());
    }
    args.push("--port".into());
    args.push(port);
    spawn_detached(&exe, &args[1..])?;
    Ok(Some(LaunchOutcome {
        // The args carry no secrets, but keep them out of the UI anyway.
        args: vec![exe.to_string_lossy().into_owned()],
        exit_code: Some(0),
        stdout: String::new(),
        stderr: String::new(),
        timed_out: false,
    }))
}

pub fn launch(config: LaunchConfig) -> Result<LaunchOutcome, String> {
    // Steam's client owns its auth via steam_api — it never touches
    // SoulLauncher.exe or the VFUN hand-off, whatever the server choice.
    if let Some(outcome) = steam_direct_launch(&config)? {
        return Ok(outcome);
    }
    if is_global(config.server.trim()) {
        // Global borrows the official VFUN launcher's live session token —
        // SoulLauncher.exe's old auth chain is dead since VFUN migrated to
        // web-SSO, so there is nothing left for it to dry-run.
        return crate::vfun::global_launch(&config);
    }
    if config.local_emulator {
        return emulator_arg_launch(&config);
    }
    if let Some(outcome) = private_direct_launch(&config)? {
        return Ok(outcome);
    }
    let exe = resolve_launcher_path(config.launcher_path.as_deref())
        .ok_or_else(|| format!("launcher-missing: {LAUNCHER_MISSING}"))?;
    let args = build_args(&config, &exe)?;
    // Fallback path (account key missing): SoulLauncher mints the key, launches
    // the game and may stay alive for the whole session — never wait on it or
    // it would be killed mid-game. Spawn detached and return immediately.
    spawn_detached(&exe, &args[1..])?;
    Ok(LaunchOutcome {
        args,
        exit_code: Some(0),
        stdout: String::new(),
        stderr: String::new(),
        timed_out: false,
    })
}

pub fn login_status() -> LoginStatus {
    LoginStatus {
        steam_name: steam_display_name(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_valid_addresses() {
        for ok in [
            "192.168.0.10:15001",
            "192.0.2.10:10000",
            "127.0.0.1:10001",
            "[::1]:15001",
            "example.com:10000",
            "host",
            "a",
        ] {
            assert!(validate_server_address(ok).is_ok(), "should accept {ok}");
        }
    }

    #[test]
    fn rejects_flag_injection_and_junk() {
        for bad in [
            "",
            "   ",
            "--evil",
            "--logout",
            "-x",
            "1.2.3.4:10000 & calc",
            "a b",
            "host:0",
            "host:65536",
            "host:99999",
            "abc-",
            ".abc",
            "host;rm",
            "host|pipe",
        ] {
            assert!(validate_server_address(bad).is_err(), "should reject {bad}");
        }
    }

    fn cfg(server: &str, source: &str) -> LaunchConfig {
        LaunchConfig {
            server: server.into(),
            install_source: source.into(),
            custom_folder: None,
            skip_version_check: false,
            launcher_path: None,
            official_servers: Default::default(),
            local_emulator: false,
        }
    }

    #[test]
    fn builds_expected_argv() {
        let exe = Path::new(r"C:\fake\SoulLauncher.exe");
        let srv = "203.0.113.7:10000";
        let mut c = cfg(srv, "steam");
        assert_eq!(build_args(&c, exe).unwrap(), vec![r"C:\fake\SoulLauncher.exe", "--server", srv, "--silent"]);

        c = cfg(srv, "vfun");
        assert_eq!(
            build_args(&c, exe).unwrap(),
            vec![r"C:\fake\SoulLauncher.exe", "--server", srv, "--vfun-install", "--silent"]
        );

        c = cfg(srv, "custom");
        c.custom_folder = Some(std::env::temp_dir().to_string_lossy().into_owned());
        let args = build_args(&c, exe).unwrap();
        assert_eq!(args[1..4], ["--server", srv, "--game"]);

        c = cfg(srv, "steam");
        c.skip_version_check = true;
        assert_eq!(
            build_args(&c, exe).unwrap(),
            vec![r"C:\fake\SoulLauncher.exe", "--server", srv, "--skip-version-check", "--silent"]
        );

        // --silent is unconditional: the field was removed, extra JSON keys are ignored.
        c = cfg(srv, "steam");
        assert_eq!(
            build_args(&c, exe).unwrap(),
            vec![r"C:\fake\SoulLauncher.exe", "--server", srv, "--silent"]
        );
    }

    #[test]
    fn emulator_argv_uses_verified_handoff() {
        let exe = Path::new(r"C:\fake\SoulWorker.exe");
        assert_eq!(
            emulator_args(exe),
            vec![r"C:\fake\SoulWorker.exe", "VFunQA:: MultiExecuteClient"]
        );
    }

    #[test]
    fn global_never_goes_through_soullauncher() {
        let exe = Path::new(r"C:\fake\SoulLauncher.exe");
        for server in ["global", "", "  "] {
            for source in ["steam", "vfun", "custom"] {
                let err = build_args(&cfg(server, source), exe).unwrap_err();
                assert!(err.starts_with("global-pending:"), "{server:?}/{source}: {err}");
            }
        }
    }

    #[test]
    fn rejects_bad_configs_with_codes() {
        let exe = Path::new(r"C:\fake\SoulLauncher.exe");
        let mut c = cfg("--logout", "steam");
        assert!(build_args(&c, exe).unwrap_err().starts_with("invalid-server:"));
        c = cfg("203.0.113.7:10000", "custom");
        assert!(build_args(&c, exe).unwrap_err().starts_with("game-not-found:"));
        c.custom_folder = Some(r"C:\no\such\folder\here".into());
        assert!(build_args(&c, exe).unwrap_err().starts_with("game-not-found:"));
    }

    #[test]
    fn splits_host_port_for_direct_launch() {
        assert_eq!(
            split_host_port("192.0.2.10:10000"),
            Some(("192.0.2.10".to_string(), "10000".to_string()))
        );
        assert_eq!(
            split_host_port("my.host.example:25565"),
            Some(("my.host.example".to_string(), "25565".to_string()))
        );
        assert_eq!(
            split_host_port("192.168.1.5"),
            Some(("192.168.1.5".to_string(), "10000".to_string()))
        );
        assert_eq!(
            split_host_port("[::1]:15001"),
            Some(("[::1]".to_string(), "15001".to_string()))
        );
        assert_eq!(split_host_port("[::1]"), None);
        assert_eq!(split_host_port("[::1]:"), None);
    }

    #[test]
    fn official_mode_flag_never_on_private_servers() {
        let exe = Path::new(r"C:\fake\SoulLauncher.exe");
        for source in ["steam", "vfun"] {
            let c = cfg("203.0.113.7:10000", source);
            let args = build_args(&c, exe).unwrap();
            assert!(!args.iter().any(|a| a == "--vfun" || a == "--steam"), "{source}: {args:?}");
        }
    }
}
