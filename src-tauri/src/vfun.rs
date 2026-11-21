//! Global (official) sign-in and launch: Ultimia talks to VFUN itself and starts
//! SoulWorker.exe with the same hand-off the official launcher uses.
//! Nothing secret is hardcoded and nothing is read from any third-party launcher:
//! our own sign-in tokens are saved with Windows DPAPI, and the hand-off language
//! defaults to English.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::launcher::{self, LaunchConfig, LaunchOutcome, OfficialServers};

const API_BASE: &str = "external-api.valofe.com";
const LOGIN_PATH: &str = "/api/vfun/login";
const REFRESH_PATH: &str = "/api/vfun/refresh_token";
const AUTH_CODE_PATH: &str = "/api/vfun/make_auth_code";

const RESULT_OK: i64 = 1;
const RESULT_EXTRA_VERIFICATION: [i64; 2] = [4056, 4072];
const RESULT_TOKEN_EXPIRED: i64 = 4010;

const SIGN_IN_NEEDED: &str =
    "vfun-signin-needed: Sign in to your VFUN account in Settings → Profile first.";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SavedLogin {
    pub user_id: String,
    pub refresh_token: String,
    #[serde(default)]
    pub access_token: String,
}

struct Tokens {
    access: String,
    refresh: String,
}

enum AuthCode {
    Code(String),
    Expired,
}

/// Best-effort wipe of credential material once it is no longer needed.
fn zeroize(s: &mut String) {
    unsafe {
        for b in s.as_bytes_mut() {
            std::ptr::write_volatile(b, 0);
        }
    }
    s.clear();
}

// ---------------------------------------------------------------------------
// Saved sign-in (our own folder, DPAPI-encrypted, no keys of our own)
// ---------------------------------------------------------------------------

/// Ultimia's own sign-in store. Independent of any third-party launcher.
fn dat_path() -> PathBuf {
    let appdata = std::env::var("APPDATA").unwrap_or_default();
    Path::new(&appdata)
        .join("UltimiaLauncher")
        .join("vfun_signin.dat")
}

fn parse_dat(bytes: &[u8]) -> Option<SavedLogin> {
    let login: SavedLogin = serde_json::from_slice(bytes).ok()?;
    if login.user_id.trim().is_empty() || login.refresh_token.trim().is_empty() {
        return None;
    }
    Some(login)
}

/// Pull `name=value` out of a list of raw `Set-Cookie:` header lines.
fn cookie_value(cookies: &[String], name: &str) -> Option<String> {
    let prefix = format!("{name}=");
    for line in cookies {
        for part in line.split(';') {
            let part = part.trim();
            if let Some(v) = part.strip_prefix(&prefix) {
                let v = v.trim();
                if !v.is_empty() {
                    return Some(v.to_string());
                }
            }
        }
    }
    None
}

pub fn load_login() -> Option<SavedLogin> {
    let bytes = std::fs::read(dat_path()).ok()?;
    let plain = dpapi_unprotect(&bytes).ok()?;
    parse_dat(&plain)
}

fn save_login(login: &SavedLogin) -> Result<(), String> {
    let json = serde_json::to_vec(login).map_err(|e| format!("vfun-local: {e}"))?;
    let blob = dpapi_protect(&json)?;
    let path = dat_path();
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("vfun-local: {e}"))?;
    }
    std::fs::write(&path, blob).map_err(|e| format!("vfun-local: Could not save the sign-in: {e}"))
}

/// Marker file: user signed out, so don't keep re-detecting the official
/// SoulLauncher's account and showing it as signed in.
fn dismissed_path() -> PathBuf {
    let appdata = std::env::var("APPDATA").unwrap_or_default();
    Path::new(&appdata)
        .join("UltimiaLauncher")
        .join("vfun_dismissed")
}

pub fn sign_out() -> Result<Vec<String>, String> {
    let mut removed = Vec::new();
    let path = dat_path();
    if path.is_file() {
        std::fs::remove_file(&path)
            .map_err(|e| format!("vfun-local: Could not remove {}: {e}", path.display()))?;
        removed.push(path.to_string_lossy().into_owned());
    }
    let marker = dismissed_path();
    if let Some(dir) = marker.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    std::fs::write(&marker, b"signed-out")
        .map_err(|e| format!("vfun-local: Could not save sign-out state: {e}"))?;
    Ok(removed)
}

// ---------------------------------------------------------------------------
// Import the official SoulLauncher's saved sign-in (read-only peek + validated copy)
// ---------------------------------------------------------------------------

/// `SoulLauncher.dat` sits next to SoulWorker.exe in each detected install.
fn soullauncher_dat_paths() -> Vec<PathBuf> {
    launcher::game_dirs()
        .into_iter()
        .map(|dir| dir.join("SoulLauncher.dat"))
        .collect()
}

fn read_soullauncher_login(path: &Path) -> Option<SavedLogin> {
    let bytes = std::fs::read(path).ok()?;
    let plain = dpapi_unprotect(&bytes).ok()?;
    parse_dat(&plain)
}

/// Account id for display: our own store first, then a read-only peek at the
/// official SoulLauncher's DPAPI file. Nothing is copied at this stage.
pub fn detected_account() -> Option<String> {
    load_login()
        .map(|l| l.user_id)
        .or_else(|| {
            if dismissed_path().is_file() {
                return None;
            }
            soullauncher_dat_paths()
                .into_iter()
                .find_map(|p| read_soullauncher_login(&p))
                .map(|l| l.user_id)
        })
}

// ---------------------------------------------------------------------------
// VFUN API
// ---------------------------------------------------------------------------

fn parse_envelope(body: &str) -> Result<(i64, serde_json::Value), String> {
    let v: serde_json::Value = serde_json::from_str(body)
        .map_err(|_| "vfun-network: VFUN sent an unreadable response.".to_string())?;
    let result = v
        .get("result")
        .and_then(serde_json::Value::as_i64)
        .ok_or_else(|| "vfun-network: VFUN response has no result code.".to_string())?;
    Ok((result, v))
}

/// Token fields may live under `data` or at the response top level — check both.
fn token_field(v: &serde_json::Value, name: &str) -> Option<String> {
    v.get("data")
        .and_then(|d| d.get(name))
        .or_else(|| v.get(name))
        .and_then(serde_json::Value::as_str)
        .map(str::to_string)
        .filter(|t| !t.is_empty())
}

fn url_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

fn api_login(user_id: &str, password: &str, device: &str) -> Result<Tokens, String> {
    // The game client itself posts form-encoded credentials to this endpoint
    // (SoulWorker64.dll: `service_code=vfun&input_user_id=%s&input_user_password=%s`).
    // That code path may still return the legacy L-Access/L-Refresh tokens the
    // game expects, unlike the JSON variant which yields `valofe_web_token`.
    let mut body = format!(
        "service_code=vfun&input_user_id={}&input_user_password={}",
        url_encode(user_id),
        url_encode(password)
    );
    let headers = [
        ("Content-Type", "application/x-www-form-urlencoded"),
        ("Device", "web"),
        ("Device-Id", device),
    ];
    let (response, cookies) = http_request("POST", LOGIN_PATH, &headers, Some(body.as_bytes()))?;
    zeroize(&mut body);
    let (result, data) = parse_envelope(&response)?;
    match result {
        RESULT_OK => {
            // The game client reads L-Access/L-Refresh from Set-Cookie
            // headers; the JSON body carries SSO fields instead. Take the
            // cookie tokens first, body fields as fallback.
            let access = cookie_value(&cookies, "L-Access-Token")
                .or_else(|| token_field(&data, "L-Access-Token"))
                .ok_or_else(|| "vfun-auth-failed: VFUN accepted the sign-in but sent no tokens.".to_string())?;
            let refresh = cookie_value(&cookies, "L-Refresh-Token")
                .or_else(|| token_field(&data, "L-Refresh-Token"))
                .ok_or_else(|| "vfun-auth-failed: VFUN accepted the sign-in but sent no tokens.".to_string())?;
            Ok(Tokens { access, refresh })
        }
        r if RESULT_EXTRA_VERIFICATION.contains(&r) => Err(
            "vfun-extra-verification: VFUN wants an extra verification step for this account. Sign in once through the official VFUN launcher with auto-login ticked, then try again.".into(),
        ),
        other => Err(format!("vfun-auth-failed: VFUN rejected the sign-in (result {other}).")),
    }
}

fn api_refresh(refresh: &str, access: &str, device: &str) -> Result<Tokens, String> {
    // The game sends two cookie pairs on refresh
    // (`Cookie: %s=%s;%s=%s;Device=%s;Device-Id=%s`): the refresh token plus
    // the current access token when we have one.
    let cookie = if access.is_empty() {
        format!("L-Refresh-Token={refresh}")
    } else {
        format!("L-Refresh-Token={refresh};L-Access-Token={access}")
    };
    let headers = [
        ("Content-Type", "application/json; charset=UTF-8"),
        ("Cookie", cookie.as_str()),
        ("Device", "web"),
        ("Device-Id", device),
    ];
    let (response, cookies) = http_request("POST", REFRESH_PATH, &headers, None)?;
    let (result, data) = parse_envelope(&response)?;
    if result != RESULT_OK {
        return Err(format!(
            "vfun-signin-needed: The saved VFUN sign-in is no longer valid (result {result}). Sign in again in Settings → Profile."
        ));
    }
    let new_access = cookie_value(&cookies, "L-Access-Token")
        .or_else(|| token_field(&data, "L-Access-Token"))
        .ok_or_else(|| "vfun-auth-failed: VFUN sent no access token.".to_string())?;
    let new_refresh = cookie_value(&cookies, "L-Refresh-Token")
        .or_else(|| token_field(&data, "L-Refresh-Token"))
        .unwrap_or_else(|| refresh.to_string());
    Ok(Tokens {
        access: new_access,
        refresh: new_refresh,
    })
}

fn api_auth_code(access: &str, device: &str) -> Result<AuthCode, String> {
    // Matches the game client's own call: `Cookie: L-Access-Token=%s;Device=web;Device-Id=%s`
    let cookie = format!("L-Access-Token={access};Device=web;Device-Id={device}");
    let headers = [
        ("Content-Type", "application/json; charset=UTF-8"),
        ("Cookie", cookie.as_str()),
    ];
    let (response, _) = http_request("GET", AUTH_CODE_PATH, &headers, None)?;
    let (result, data) = parse_envelope(&response)?;
    match result {
        RESULT_OK => token_field(&data, "auth_code")
            .map(AuthCode::Code)
            .ok_or_else(|| "vfun-auth-failed: VFUN sent no game auth code.".to_string()),
        RESULT_TOKEN_EXPIRED => Ok(AuthCode::Expired),
        other => Err(format!("vfun-auth-failed: VFUN refused a game auth code (result {other}).")),
    }
}

/// Sign in with a VFUN ID and password. The password goes to VFUN only; it is
/// never written to disk, logs, or settings. Only the returned tokens are kept,
/// DPAPI-encrypted, in Ultimia's own folder.
pub fn sign_in(user_id: &str, mut password: String) -> Result<String, String> {
    let user_id = user_id.trim();
    if user_id.is_empty() || password.is_empty() {
        return Err("vfun-auth-failed: Enter your VFUN ID and password.".into());
    }
    let device = device_id()?;
    let tokens = api_login(user_id, &password, &device);
    zeroize(&mut password);
    let tokens = tokens?;
    let login = SavedLogin {
        user_id: user_id.to_string(),
        refresh_token: tokens.refresh,
        access_token: tokens.access,
    };
    save_login(&login)?;
    let _ = std::fs::remove_file(dismissed_path());
    Ok(login.user_id)
}

// ---------------------------------------------------------------------------
// Global launch
// ---------------------------------------------------------------------------

fn handoff_string(code: &str, plan: &str, user_id: &str, servers: &OfficialServers) -> String {
    format!(
        "fromVLauncher::VALOFE:{code}:gbl:{}:gbl:{}:chn:{}:{plan}:{user_id}",
        servers.global1, servers.global2, servers.china
    )
}

pub(crate) fn game_exe(config: &LaunchConfig) -> Result<PathBuf, String> {
    let dir = match config.install_source.as_str() {
        "custom" => config
            .custom_folder
            .as_deref()
            .map(str::trim)
            .filter(|f| !f.is_empty())
            .map(PathBuf::from)
            .ok_or_else(|| "game-not-found: Choose a game folder first.".to_string())?,
        _ => launcher::game_dirs()
            .into_iter()
            .next()
            .ok_or_else(|| "game-not-found: No SoulWorker installation found.".to_string())?,
    };
    let exe = dir.join("SoulWorker.exe");
    if !exe.is_file() {
        return Err(format!("game-not-found: SoulWorker.exe is not in {}.", dir.display()));
    }
    Ok(exe)
}

/// Global launch, fully independent of VFUN Launcher: refresh the saved
/// sign-in (the same calls the game client makes), mint a game auth code,
/// and start SoulWorker.exe with the official `VALOFE` hand-off. Requires a
/// one-time sign-in in Settings → Profile; nothing else is needed.
pub fn global_launch(config: &LaunchConfig) -> Result<LaunchOutcome, String> {
    for server in [&config.official_servers.global1, &config.official_servers.global2, &config.official_servers.china] {
        launcher::validate_server_address(server).map_err(|e| format!("invalid-server: {e}"))?;
    }
    let device = device_id()?;
    let mut login = load_login().ok_or_else(|| SIGN_IN_NEEDED.to_string())?;

    let mut tokens = api_refresh(&login.refresh_token, &login.access_token, &device)?;
    login.refresh_token = tokens.refresh.clone();
    login.access_token = tokens.access.clone();
    let _ = save_login(&login);

    let mut code = match api_auth_code(&tokens.access, &device)? {
        AuthCode::Code(c) => c,
        AuthCode::Expired => {
            // Access token raced expiry — refresh once more and retry.
            tokens = api_refresh(&login.refresh_token, &login.access_token, &device)?;
            login.refresh_token = tokens.refresh.clone();
            login.access_token = tokens.access.clone();
            let _ = save_login(&login);
            match api_auth_code(&tokens.access, &device)? {
                AuthCode::Code(c) => c,
                AuthCode::Expired => {
                    return Err("vfun-auth-failed: VFUN kept rejecting the game auth code.".into())
                }
            }
        }
    };
    let user_id = login.user_id.clone();
    zeroize(&mut tokens.access);
    zeroize(&mut tokens.refresh);

    // ULTIMIA_GBL_RELAY=host:port reroutes every official server slot in the
    // hand-off through a local relay (e.g. the sniffer) for traffic capture.
    let servers = match std::env::var("ULTIMIA_GBL_RELAY")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
    {
        Some(relay) => OfficialServers {
            global1: relay.clone(),
            global2: relay.clone(),
            china: relay,
        },
        None => config.official_servers.clone(),
    };
    let mut handoff = handoff_string(&code, "English", &user_id, &servers);
    zeroize(&mut code);

    let exe = game_exe(config)?;
    let dir = exe.parent().unwrap_or(Path::new("."));
    let started = shell_execute_open(&exe, &handoff, dir);
    zeroize(&mut handoff);
    started?;

    // The official launcher stamps this only for VFUN installs; the game reads it.
    if config.install_source == "vfun" {
        write_funtime();
    }

    // The hand-off carries a short-lived auth code, so it never reaches the log.
    Ok(LaunchOutcome {
        args: vec![exe.to_string_lossy().into_owned()],
        exit_code: Some(0),
        stdout: String::new(),
        stderr: String::new(),
        timed_out: false,
    })
}

// ---------------------------------------------------------------------------
// Windows plumbing (DPAPI, BCrypt, WMI, WinHTTP, shell)
// ---------------------------------------------------------------------------

#[cfg(windows)]
fn dpapi_protect(data: &[u8]) -> Result<Vec<u8>, String> {
    use windows::core::PCWSTR;
    use windows::Win32::Foundation::{HLOCAL, LocalFree};
    use windows::Win32::Security::Cryptography::{
        CryptProtectData, CRYPT_INTEGER_BLOB, CRYPTPROTECT_UI_FORBIDDEN,
    };

    let input = CRYPT_INTEGER_BLOB {
        cbData: data.len() as u32,
        pbData: data.as_ptr() as *mut u8,
    };
    let desc: Vec<u16> = "UltimiaLauncher".encode_utf16().chain(std::iter::once(0)).collect();
    let mut out = CRYPT_INTEGER_BLOB::default();
    unsafe {
        CryptProtectData(
            &input,
            PCWSTR(desc.as_ptr()),
            None,
            None,
            None,
            CRYPTPROTECT_UI_FORBIDDEN,
            &mut out,
        )
        .map_err(|e| format!("vfun-local: Windows could not encrypt the sign-in: {e}"))?;
        let bytes = std::slice::from_raw_parts(out.pbData, out.cbData as usize).to_vec();
        let _ = LocalFree(Some(HLOCAL(out.pbData as *mut _)));
        Ok(bytes)
    }
}

#[cfg(windows)]
fn dpapi_unprotect(blob: &[u8]) -> Result<Vec<u8>, String> {
    use windows::Win32::Foundation::{HLOCAL, LocalFree};
    use windows::Win32::Security::Cryptography::{
        CryptUnprotectData, CRYPT_INTEGER_BLOB, CRYPTPROTECT_UI_FORBIDDEN,
    };

    let input = CRYPT_INTEGER_BLOB {
        cbData: blob.len() as u32,
        pbData: blob.as_ptr() as *mut u8,
    };
    let mut out = CRYPT_INTEGER_BLOB::default();
    unsafe {
        CryptUnprotectData(
            &input,
            None,
            None,
            None,
            None,
            CRYPTPROTECT_UI_FORBIDDEN,
            &mut out,
        )
        .map_err(|e| format!("vfun-local: Windows could not decrypt the saved sign-in: {e}"))?;
        let bytes = std::slice::from_raw_parts(out.pbData, out.cbData as usize).to_vec();
        let _ = LocalFree(Some(HLOCAL(out.pbData as *mut _)));
        Ok(bytes)
    }
}

/// The official launcher's device id is the WMI machine UUID verbatim —
/// proven by the `device_id` claim inside the JWT stored in
/// `SoulLauncher.dat`, which matches `Win32_ComputerSystemProduct.UUID` exactly.
/// Computed once per run; never logged or stored.
#[cfg(windows)]
fn device_id() -> Result<String, String> {
    use std::sync::OnceLock;
    static ID: OnceLock<Result<String, String>> = OnceLock::new();
    ID.get_or_init(wmi_uuid).clone()
}

#[cfg(windows)]
fn wmi_uuid() -> Result<String, String> {
    use windows::core::BSTR;
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CoSetProxyBlanket, CoUninitialize,
        CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, EOAC_NONE, RPC_C_AUTHN_LEVEL_CALL,
        RPC_C_IMP_LEVEL_IMPERSONATE,
    };
    use windows::Win32::System::Rpc::{RPC_C_AUTHN_WINNT, RPC_C_AUTHZ_NONE};
    use windows::Win32::System::Variant::{VariantClear, VARIANT, VT_BSTR};
    use windows::Win32::System::Wmi::{
        IEnumWbemClassObject, IWbemClassObject, IWbemLocator, WbemLocator,
        WBEM_FLAG_FORWARD_ONLY, WBEM_FLAG_RETURN_IMMEDIATELY, WBEM_INFINITE,
    };

    unsafe {
        let mut com_owned = false;
        let hr = CoInitializeEx(None, COINIT_MULTITHREADED);
        if hr.is_ok() {
            com_owned = true;
        } else if hr.0 as u32 != 0x80010106 {
            // RPC_E_CHANGED_MODE: another apartment mode on this thread is fine to use,
            // just not ours to close.
            return Err(format!("vfun-local: COM init failed (0x{:08X}).", hr.0 as u32));
        }
        let result = (|| {
            let locator: IWbemLocator =
                CoCreateInstance(&WbemLocator, None, CLSCTX_INPROC_SERVER)
                    .map_err(|e| format!("vfun-local: WMI locator failed: {e}"))?;
            let services = locator
                .ConnectServer(
                    &BSTR::from("ROOT\\CIMV2"),
                    &BSTR::new(),
                    &BSTR::new(),
                    &BSTR::new(),
                    0,
                    &BSTR::new(),
                    None,
                )
                .map_err(|e| format!("vfun-local: WMI connect failed: {e}"))?;
            CoSetProxyBlanket(
                &services,
                RPC_C_AUTHN_WINNT,
                RPC_C_AUTHZ_NONE,
                None,
                RPC_C_AUTHN_LEVEL_CALL,
                RPC_C_IMP_LEVEL_IMPERSONATE,
                None,
                EOAC_NONE,
            )
            .map_err(|e| format!("vfun-local: WMI security failed: {e}"))?;
            let query: IEnumWbemClassObject = services
                .ExecQuery(
                    &BSTR::from("WQL"),
                    &BSTR::from("SELECT UUID FROM Win32_ComputerSystemProduct"),
                    WBEM_FLAG_FORWARD_ONLY | WBEM_FLAG_RETURN_IMMEDIATELY,
                    None,
                )
                .map_err(|e| format!("vfun-local: WMI query failed: {e}"))?;
            let mut objects: [Option<IWbemClassObject>; 1] = [None];
            let mut returned = 0u32;
            let hr = query.Next(WBEM_INFINITE, &mut objects, &mut returned);
            if hr.is_err() || returned == 0 {
                return Err("vfun-local: WMI returned no machine UUID.".to_string());
            }
            let object = objects[0]
                .as_ref()
                .ok_or_else(|| "vfun-local: WMI returned no machine UUID.".to_string())?;
            let mut value = VARIANT::default();
            let got = object.Get(windows::core::w!("UUID"), 0, &mut value, None, None);
            let uuid = if got.is_ok() && value.Anonymous.Anonymous.vt == VT_BSTR {
                Some(value.Anonymous.Anonymous.Anonymous.bstrVal.to_string())
            } else {
                None
            };
            let _ = VariantClear(&mut value);
            uuid.filter(|u| !u.is_empty())
                .ok_or_else(|| "vfun-local: WMI returned an empty machine UUID.".to_string())
        })();
        if com_owned {
            CoUninitialize();
        }
        result
    }
}

#[cfg(windows)]
fn http_request(
    method: &str,
    path: &str,
    headers: &[(&str, &str)],
    body: Option<&[u8]>,
) -> Result<(String, Vec<String>), String> {
    use windows::core::PCWSTR;
    use windows::Win32::Networking::WinHttp::{
        WinHttpCloseHandle, WinHttpConnect, WinHttpOpen, WinHttpOpenRequest, WinHttpQueryHeaders,
        WinHttpReadData, WinHttpReceiveResponse, WinHttpSendRequest, WinHttpSetTimeouts,
        WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY, WINHTTP_FLAG_SECURE, WINHTTP_QUERY_FLAG_NUMBER,
        WINHTTP_QUERY_RAW_HEADERS_CRLF, WINHTTP_QUERY_STATUS_CODE,
    };

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }
    fn last_error(what: &str) -> String {
        let code = unsafe { windows::Win32::Foundation::GetLastError().0 };
        format!("vfun-network: {what} (Windows error {code}).")
    }

    let agent = wide("UltimiaLauncher");
    let host = wide(API_BASE);
    let verb = wide(method);
    let object = wide(path);
    let header_block: String = headers.iter().map(|(k, v)| format!("{k}: {v}\r\n")).collect();
    let header_w: Vec<u16> = header_block.encode_utf16().collect();

    unsafe {
        let session = WinHttpOpen(
            PCWSTR(agent.as_ptr()),
            WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY,
            PCWSTR::null(),
            PCWSTR::null(),
            0,
        );
        if session.is_null() {
            return Err(last_error("Could not open a VFUN session"));
        }
        let _ = WinHttpSetTimeouts(session, 15000, 15000, 15000, 15000);
        let connect = WinHttpConnect(session, PCWSTR(host.as_ptr()), 443, 0);
        if connect.is_null() {
            let _ = WinHttpCloseHandle(session);
            return Err(last_error("Could not reach VFUN"));
        }
        let request = WinHttpOpenRequest(
            connect,
            PCWSTR(verb.as_ptr()),
            PCWSTR(object.as_ptr()),
            PCWSTR::null(),
            PCWSTR::null(),
            std::ptr::null(),
            WINHTTP_FLAG_SECURE,
        );
        if request.is_null() {
            let _ = WinHttpCloseHandle(connect);
            let _ = WinHttpCloseHandle(session);
            return Err(last_error("Could not create the VFUN request"));
        }

        let body_len = body.map(|b| b.len()).unwrap_or(0) as u32;
        let sent = WinHttpSendRequest(
            request,
            if header_w.is_empty() { None } else { Some(&header_w) },
            body.map(|b| b.as_ptr() as *const _),
            body_len,
            body_len,
            0,
        );
        let result = if sent.is_err() {
            Err(last_error("Could not send the VFUN request"))
        } else if WinHttpReceiveResponse(request, std::ptr::null_mut()).is_err() {
            Err(last_error("VFUN did not answer"))
        } else {
            let mut status = 0u32;
            let mut size = std::mem::size_of::<u32>() as u32;
            let queried = WinHttpQueryHeaders(
                request,
                WINHTTP_QUERY_STATUS_CODE | WINHTTP_QUERY_FLAG_NUMBER,
                PCWSTR::null(),
                Some(&mut status as *mut u32 as *mut _),
                &mut size,
                std::ptr::null_mut(),
            );
            if queried.is_err() {
                Err(last_error("Could not read the VFUN response status"))
            } else if status != 200 {
                Err(format!("vfun-network: VFUN answered HTTP {status}."))
            } else {
                // Grab Set-Cookie lines: the game client takes L-Access /
                // L-Refresh tokens from cookies, not the JSON body.
                let mut cookies: Vec<String> = Vec::new();
                let mut hlen = 0u32;
                let _ = WinHttpQueryHeaders(
                    request,
                    WINHTTP_QUERY_RAW_HEADERS_CRLF,
                    PCWSTR::null(),
                    None,
                    &mut hlen,
                    std::ptr::null_mut(),
                );
                if hlen > 0 {
                    let mut hbuf = vec![0u16; (hlen as usize).div_ceil(2) + 2];
                    let mut hsize = hbuf.len() as u32 * 2;
                    if WinHttpQueryHeaders(
                        request,
                        WINHTTP_QUERY_RAW_HEADERS_CRLF,
                        PCWSTR::null(),
                        Some(hbuf.as_mut_ptr() as *mut _),
                        &mut hsize,
                        std::ptr::null_mut(),
                    )
                    .is_ok()
                    {
                        let raw = String::from_utf16_lossy(
                            &hbuf[..(hsize as usize) / 2],
                        );
                        for line in raw.lines() {
                            let l = line.trim();
                            if l.len() >= 11 && l[..11].eq_ignore_ascii_case("set-cookie:") {
                                cookies.push(l[11..].trim().to_string());
                            }
                        }
                    }
                }
                let mut out = Vec::new();
                let mut chunk = [0u8; 8192];
                loop {
                    let mut read = 0u32;
                    if WinHttpReadData(
                        request,
                        chunk.as_mut_ptr() as *mut _,
                        chunk.len() as u32,
                        &mut read,
                    )
                    .is_err()
                    {
                        return Err(last_error("Could not read the VFUN response"));
                    }
                    if read == 0 {
                        break;
                    }
                    out.extend_from_slice(&chunk[..read as usize]);
                    if out.len() > 1024 * 1024 {
                        return Err("vfun-network: VFUN response is too large.".to_string());
                    }
                }
                Ok((String::from_utf8_lossy(&out).into_owned(), cookies))
            }
        };
        let _ = WinHttpCloseHandle(request);
        let _ = WinHttpCloseHandle(connect);
        let _ = WinHttpCloseHandle(session);
        result
    }
}

#[cfg(windows)]
pub(crate) fn shell_execute_open(exe: &Path, parameters: &str, dir: &Path) -> Result<(), String> {
    use windows::core::PCWSTR;
    use windows::Win32::UI::Shell::{ShellExecuteExW, SHELLEXECUTEINFOW};

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }
    let file = wide(&exe.to_string_lossy());
    let params = wide(parameters);
    let directory = wide(&dir.to_string_lossy());
    let mut info = SHELLEXECUTEINFOW {
        cbSize: std::mem::size_of::<SHELLEXECUTEINFOW>() as u32,
        lpFile: PCWSTR(file.as_ptr()),
        lpParameters: PCWSTR(params.as_ptr()),
        lpDirectory: PCWSTR(directory.as_ptr()),
        nShow: 1, // SW_SHOWNORMAL
        ..Default::default()
    };
    if unsafe { ShellExecuteExW(&mut info) }.is_ok() {
        Ok(())
    } else {
        let code = unsafe { windows::Win32::Foundation::GetLastError().0 };
        Err(format!("launch-failed: Windows could not start SoulWorker.exe (error {code})."))
    }
}

#[cfg(windows)]
fn write_funtime() {
    use windows::core::PCWSTR;
    use windows::Win32::System::Registry::{RegSetKeyValueW, HKEY_CURRENT_USER, REG_QWORD};
    use windows::Win32::System::SystemInformation::GetSystemTimeAsFileTime;

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }
    unsafe {
        let ft = GetSystemTimeAsFileTime();
        let ticks = ((ft.dwHighDateTime as u64) << 32 | ft.dwLowDateTime as u64)
            .wrapping_sub(116_444_736_000_000_000);
        let subkey = wide(r"Software\Valofe\SoulWorker");
        let name = wide("FUNTIME");
        let _ = RegSetKeyValueW(
            HKEY_CURRENT_USER,
            PCWSTR(subkey.as_ptr()),
            PCWSTR(name.as_ptr()),
            REG_QWORD.0,
            Some(&ticks as *const u64 as *const _),
            8,
        );
    }
}

// ---------------------------------------------------------------------------
// Non-Windows stubs (the app ships for Windows only)
// ---------------------------------------------------------------------------

#[cfg(not(windows))]
fn dpapi_protect(_data: &[u8]) -> Result<Vec<u8>, String> {
    Err("vfun-local: Global sign-in needs Windows.".into())
}

#[cfg(not(windows))]
fn dpapi_unprotect(_blob: &[u8]) -> Result<Vec<u8>, String> {
    Err("vfun-local: Global sign-in needs Windows.".into())
}

#[cfg(not(windows))]
fn device_id() -> Result<String, String> {
    Err("vfun-local: Global sign-in needs Windows.".into())
}

#[cfg(not(windows))]
fn http_request(
    _method: &str,
    _path: &str,
    _headers: &[(&str, &str)],
    _body: Option<&[u8]>,
) -> Result<(String, Vec<String>), String> {
    Err("vfun-local: Global sign-in needs Windows.".into())
}

#[cfg(not(windows))]
fn shell_execute_open(_exe: &Path, _parameters: &str, _dir: &Path) -> Result<(), String> {
    Err("launch-failed: Global launch needs Windows.".into())
}

#[cfg(not(windows))]
fn write_funtime() {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn handoff_matches_official_template() {
        let servers = OfficialServers::default();
        let h = handoff_string("CODE123", "English", "player1", &servers);
        assert_eq!(
            h,
            "fromVLauncher::VALOFE:CODE123:gbl:43.159.147.134:10000:gbl:43.130.7.93:10000:chn:49.233.245.180:10000:English:player1"
        );
    }

    #[test]
    fn handoff_uses_edited_servers() {
        let servers = OfficialServers {
            global1: "1.2.3.4:10000".into(),
            global2: "5.6.7.8:10000".into(),
            china: "9.10.11.12:10000".into(),
        };
        let h = handoff_string("C", "Taiwan", "u", &servers);
        assert!(h.contains(":gbl:1.2.3.4:10000:gbl:5.6.7.8:10000:chn:9.10.11.12:10000:Taiwan:u"));
    }

    #[test]
    fn envelope_reads_result_and_data() {
        let (r, d) = parse_envelope(r#"{"result":1,"data":{"auth_code":"x"}}"#).unwrap();
        assert_eq!(r, 1);
        assert_eq!(token_field(&d, "auth_code").as_deref(), Some("x"));
        let (r, d) = parse_envelope(r#"{"result":4010}"#).unwrap();
        assert_eq!(r, 4010);
        assert!(d.get("data").is_none());
        assert!(parse_envelope("not json").is_err());
        assert!(parse_envelope(r#"{"data":{}}"#).is_err());
    }

    #[test]
    fn dat_round_trips_through_json() {
        let login = SavedLogin {
            user_id: "player1".into(),
            refresh_token: "tok".into(),
            access_token: String::new(),
        };
        let bytes = serde_json::to_vec(&login).unwrap();
        let back = parse_dat(&bytes).unwrap();
        assert_eq!(back.user_id, "player1");
        assert_eq!(back.refresh_token, "tok");
        assert!(parse_dat(b"{}").is_none());
        assert!(parse_dat(b"{\"user_id\":\"\",\"refresh_token\":\"t\"}").is_none());
        assert!(parse_dat(b"garbage").is_none());
    }

    #[test]
    fn dat_path_is_our_own_folder() {
        let p = dat_path();
        let s = p.to_string_lossy();
        assert!(s.contains("UltimiaLauncher"), "own folder: {s}");
        assert!(!s.contains("SoulLauncher"), "no third-party launcher path: {s}");
        assert!(p.ends_with("vfun_signin.dat"));
    }

    #[test]
    fn zeroize_clears_the_buffer() {
        let mut s = String::from("secret-password");
        zeroize(&mut s);
        assert!(s.is_empty());
    }

    #[cfg(windows)]
    #[test]
    fn dpapi_round_trips() {
        let secret = b"ultimia-dpapi-test";
        let blob = dpapi_protect(secret).unwrap();
        assert_ne!(blob, secret);
        let back = dpapi_unprotect(&blob).unwrap();
        assert_eq!(back, secret);
    }

    #[cfg(windows)]
    #[test]
    fn device_id_is_the_machine_uuid() {
        let id = device_id().unwrap();
        // GUID shape: 8-4-4-4-12 hex, matching the JWT device_id claim.
        let parts: Vec<&str> = id.split('-').collect();
        assert_eq!(parts.len(), 5, "uuid shape: {id}");
        assert_eq!(parts[0].len(), 8);
        assert!(parts.iter().all(|p| p.chars().all(|c| c.is_ascii_hexdigit())));
    }
}