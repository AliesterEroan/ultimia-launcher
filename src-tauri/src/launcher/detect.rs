use std::path::{Path, PathBuf};

use super::*;

const STEAM_APP_ID: &str = "1377580";

pub fn resolve_launcher_path(override_path: Option<&str>) -> Option<PathBuf> {
    if let Some(p) = override_path.filter(|p| !p.trim().is_empty()) {
        let cand = PathBuf::from(p);
        if cand.is_file() {
            return Some(cand);
        }
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let cand = dir.join("SoulLauncher.exe");
            if cand.is_file() {
                return Some(cand);
            }
        }
    }
    game_dirs()
        .into_iter()
        .map(|dir| dir.join("SoulLauncher.exe"))
        .find(|cand| cand.is_file())
}

#[cfg(windows)]
pub(crate) fn reg_string(subkey: &str, value: &str) -> Option<String> {
    use windows::core::PCWSTR;
    use windows::Win32::Foundation::ERROR_SUCCESS;
    use windows::Win32::System::Registry::{RegGetValueW, HKEY_CURRENT_USER, RRF_RT_REG_SZ};

    let key: Vec<u16> = subkey.encode_utf16().chain(std::iter::once(0)).collect();
    let name: Vec<u16> = value.encode_utf16().chain(std::iter::once(0)).collect();
    let mut size = 0u32;
    unsafe {
        let probe = RegGetValueW(
            HKEY_CURRENT_USER,
            PCWSTR(key.as_ptr()),
            PCWSTR(name.as_ptr()),
            RRF_RT_REG_SZ,
            None,
            None,
            Some(&mut size),
        );
        if probe != ERROR_SUCCESS || size == 0 {
            return None;
        }
        let mut buf = vec![0u16; (size as usize).div_ceil(2)];
        let read = RegGetValueW(
            HKEY_CURRENT_USER,
            PCWSTR(key.as_ptr()),
            PCWSTR(name.as_ptr()),
            RRF_RT_REG_SZ,
            None,
            Some(buf.as_mut_ptr() as *mut _),
            Some(&mut size),
        );
        if read != ERROR_SUCCESS {
            return None;
        }
        let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
        let s = String::from_utf16_lossy(&buf[..len]).trim().to_string();
        (!s.is_empty()).then_some(s)
    }
}

#[cfg(not(windows))]
pub(crate) fn reg_string(_subkey: &str, _value: &str) -> Option<String> {
    None
}

enum Vdf {
    Str(String),
    Obj(Vec<(String, Vdf)>),
}

enum VdfTok {
    S(String),
    Open,
    Close,
}

fn vdf_tokens(text: &str) -> Vec<VdfTok> {
    let mut out = Vec::new();
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '{' => out.push(VdfTok::Open),
            '}' => out.push(VdfTok::Close),
            '"' => {
                let mut s = String::new();
                while let Some(c) = chars.next() {
                    match c {
                        '\\' => {
                            if let Some(n) = chars.next() {
                                s.push(match n {
                                    'n' => '\n',
                                    't' => '\t',
                                    other => other,
                                });
                            }
                        }
                        '"' => break,
                        other => s.push(other),
                    }
                }
                out.push(VdfTok::S(s));
            }
            '/' if chars.peek() == Some(&'/') => {
                for c in chars.by_ref() {
                    if c == '\n' {
                        break;
                    }
                }
            }
            _ => {}
        }
    }
    out
}

fn vdf_obj(toks: &[VdfTok], i: &mut usize) -> Vec<(String, Vdf)> {
    let mut items = Vec::new();
    while *i < toks.len() {
        match &toks[*i] {
            VdfTok::Close => {
                *i += 1;
                break;
            }
            VdfTok::Open => *i += 1,
            VdfTok::S(key) => {
                let key = key.clone();
                *i += 1;
                match toks.get(*i) {
                    Some(VdfTok::S(v)) => {
                        items.push((key, Vdf::Str(v.clone())));
                        *i += 1;
                    }
                    Some(VdfTok::Open) => {
                        *i += 1;
                        let child = vdf_obj(toks, i);
                        items.push((key, Vdf::Obj(child)));
                    }
                    _ => {}
                }
            }
        }
    }
    items
}

fn vdf_parse(text: &str) -> Vec<(String, Vdf)> {
    let toks = vdf_tokens(text);
    let mut i = 0;
    vdf_obj(&toks, &mut i)
}

fn vdf_get<'a>(items: &'a [(String, Vdf)], key: &str) -> Option<&'a Vdf> {
    items
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case(key))
        .map(|(_, v)| v)
}

fn vdf_str<'a>(items: &'a [(String, Vdf)], key: &str) -> Option<&'a str> {
    match vdf_get(items, key)? {
        Vdf::Str(s) => Some(s.as_str()),
        Vdf::Obj(_) => None,
    }
}

fn vdf_children<'a>(items: &'a [(String, Vdf)], key: &str) -> &'a [(String, Vdf)] {
    match vdf_get(items, key) {
        Some(Vdf::Obj(children)) => children,
        _ => &[],
    }
}

fn library_paths(libraryfolders: &str) -> Vec<PathBuf> {
    let root = vdf_parse(libraryfolders);
    vdf_children(&root, "libraryfolders")
        .iter()
        .filter_map(|(key, value)| match value {
            Vdf::Obj(entry) => vdf_str(entry, "path").map(PathBuf::from),
            // Pre-2021 Steam wrote numbered keys holding the path directly.
            Vdf::Str(path) if key.chars().all(|c| c.is_ascii_digit()) => Some(PathBuf::from(path)),
            Vdf::Str(_) => None,
        })
        .collect()
}

fn manifest_install_dir(appmanifest: &str) -> Option<String> {
    let root = vdf_parse(appmanifest);
    vdf_str(vdf_children(&root, "AppState"), "installdir")
        .map(str::to_string)
        .filter(|d| !d.trim().is_empty())
}

/// Display name of the Steam account that signs in automatically; never the login name.
fn steam_persona(loginusers: &str, auto_login_user: Option<&str>) -> Option<String> {
    let root = vdf_parse(loginusers);
    let users = vdf_children(&root, "users");
    let entries: Vec<&[(String, Vdf)]> = users
        .iter()
        .filter_map(|(_, v)| match v {
            Vdf::Obj(entry) => Some(entry.as_slice()),
            Vdf::Str(_) => None,
        })
        .collect();
    let pick = auto_login_user
        .and_then(|user| {
            entries.iter().find(|e| {
                vdf_str(e, "AccountName").is_some_and(|name| name.eq_ignore_ascii_case(user))
            })
        })
        .or_else(|| entries.iter().find(|e| vdf_str(e, "MostRecent") == Some("1")))
        .or(if entries.len() == 1 { entries.first() } else { None })?;
    vdf_str(pick, "PersonaName")
        .map(str::trim)
        .filter(|n| !n.is_empty())
        .map(str::to_string)
}

fn steam_root() -> Option<PathBuf> {
    reg_string(r"Software\Valve\Steam", "SteamPath")
        .map(PathBuf::from)
        .filter(|p| p.is_dir())
}

fn path_key(p: &Path) -> String {
    p.to_string_lossy()
        .replace('/', "\\")
        .trim_end_matches('\\')
        .to_lowercase()
}

pub(super) fn steam_game_dir() -> Option<PathBuf> {
    let root = steam_root()?;
    let mut libraries = vec![root.clone()];
    if let Ok(text) = std::fs::read_to_string(root.join("steamapps").join("libraryfolders.vdf")) {
        for lib in library_paths(&text) {
            if !libraries.iter().any(|l| path_key(l) == path_key(&lib)) {
                libraries.push(lib);
            }
        }
    }
    libraries.into_iter().find_map(|lib| {
        let steamapps = lib.join("steamapps");
        let manifest =
            std::fs::read_to_string(steamapps.join(format!("appmanifest_{STEAM_APP_ID}.acf"))).ok()?;
        let dir = steamapps.join("common").join(manifest_install_dir(&manifest)?);
        dir.is_dir().then_some(dir)
    })
}

fn vfun_game_dir() -> Option<PathBuf> {
    reg_string(r"Software\Valofe\SoulWorker", "PATH")
        .map(PathBuf::from)
        .filter(|p| p.is_dir())
}

/// SoulWorker folders this PC reports, VFUN first; nothing is assumed about drive or folder names.
pub fn game_dirs() -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = Vec::new();
    for dir in [vfun_game_dir(), steam_game_dir()].into_iter().flatten() {
        if !dirs.iter().any(|d| path_key(d) == path_key(&dir)) {
            dirs.push(dir);
        }
    }
    dirs
}

pub(super) fn steam_display_name() -> Option<String> {
    let root = steam_root()?;
    let text = std::fs::read_to_string(root.join("config").join("loginusers.vdf")).ok()?;
    steam_persona(&text, reg_string(r"Software\Valve\Steam", "AutoLoginUser").as_deref())
}

/// Install detection without spawning SoulLauncher.exe — `game_dirs()` reads
/// the registry and Steam manifests directly, so no launcher window can appear.
/// `server` stays empty: it needs SoulLauncher's own state.
pub fn detect_install() -> StatusReport {
    let launcher_path = resolve_launcher_path(None);
    let launcher_found = launcher_path.is_some();
    let dirs = game_dirs();
    let installs: Vec<String> = dirs
        .iter()
        .map(|dir| {
            let is_vfun = vfun_game_dir()
                .map(|v| path_key(&v) == path_key(dir))
                .unwrap_or(false);
            format!("{} - {}", if is_vfun { "VFUN" } else { "Steam" }, dir.display())
        })
        .collect();
    let install = installs.first().cloned();
    StatusReport {
        installs,
        launcher_path: launcher_path.map(|p| p.to_string_lossy().into_owned()),
        found: launcher_found,
        server: None,
        install,
        error: None,
    }
}

#[cfg(windows)]
pub fn client_version(install_dir: Option<String>) -> Option<String> {
    use windows::core::PCWSTR;
    use windows::Win32::Storage::FileSystem::{
        GetFileVersionInfoSizeW, GetFileVersionInfoW, VerQueryValueW, VS_FIXEDFILEINFO,
    };

    let exe = install_dir
        .filter(|d| !d.trim().is_empty())
        .map(PathBuf::from)
        .into_iter()
        .chain(game_dirs())
        .map(|dir| dir.join("SoulWorker.exe"))
        .find(|exe| exe.is_file())?;
    let path_w: Vec<u16> = exe
        .to_string_lossy()
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();
    unsafe {
        let size = GetFileVersionInfoSizeW(PCWSTR(path_w.as_ptr()), None);
        if size == 0 {
            return None;
        }
        let mut buf = vec![0u8; size as usize];
        GetFileVersionInfoW(PCWSTR(path_w.as_ptr()), Some(0), size, buf.as_mut_ptr() as *mut _).ok()?;
        let mut len = 0u32;
        let mut ptr: *mut core::ffi::c_void = std::ptr::null_mut();
        let root: Vec<u16> = "\\".encode_utf16().chain(std::iter::once(0)).collect();
        if !VerQueryValueW(buf.as_ptr() as *const _, PCWSTR(root.as_ptr()), &mut ptr, &mut len).as_bool() {
            return None;
        }
        if ptr.is_null() || (len as usize) < std::mem::size_of::<VS_FIXEDFILEINFO>() {
            return None;
        }
        let info = &*(ptr as *const VS_FIXEDFILEINFO);
        Some(format!(
            "{}.{}.{}.{}",
            info.dwFileVersionMS >> 16,
            info.dwFileVersionMS & 0xFFFF,
            info.dwFileVersionLS >> 16,
            info.dwFileVersionLS & 0xFFFF
        ))
    }
}

#[cfg(not(windows))]
pub fn client_version(_install_dir: Option<String>) -> Option<String> {
    None
}

#[cfg(windows)]
pub fn process_running(name: &str) -> bool {
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
        TH32CS_SNAPPROCESS,
    };

    unsafe {
        let Ok(snapshot) = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) else {
            return false;
        };
        let mut entry = PROCESSENTRY32W {
            dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
            ..Default::default()
        };
        let mut found = false;
        if Process32FirstW(snapshot, &mut entry).is_ok() {
            loop {
                let len = entry
                    .szExeFile
                    .iter()
                    .position(|&c| c == 0)
                    .unwrap_or(entry.szExeFile.len());
                if String::from_utf16_lossy(&entry.szExeFile[..len]).eq_ignore_ascii_case(name) {
                    found = true;
                    break;
                }
                if Process32NextW(snapshot, &mut entry).is_err() {
                    break;
                }
            }
        }
        let _ = CloseHandle(snapshot);
        found
    }
}

#[cfg(not(windows))]
pub fn process_running(_name: &str) -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_steam_libraries_both_formats() {
        let modern = r#""libraryfolders"
{
	"0"
	{
		"path"		"C:\\Program Files (x86)\\Steam"
		"apps" { "228980" "1" }
	}
	"1"
	{
		"path"		"D:\\SteamLibrary"
	}
}"#;
        assert_eq!(
            library_paths(modern),
            vec![PathBuf::from(r"C:\Program Files (x86)\Steam"), PathBuf::from(r"D:\SteamLibrary")]
        );
        let legacy = "\"LibraryFolders\"\n{\n\t\"TimeNextStatsReport\"\t\"1\"\n\t\"1\"\t\"E:\\\\Games\\\\Steam\"\n}";
        assert_eq!(library_paths(legacy), vec![PathBuf::from(r"E:\Games\Steam")]);
        assert_eq!(
            manifest_install_dir("\"AppState\"\n{\n\t\"appid\"\t\"1377580\"\n\t\"installdir\"\t\"Soulworker\"\n}").as_deref(),
            Some("Soulworker")
        );
    }

    #[test]
    fn steam_persona_uses_auto_login_user_only() {
        let users = r#""users"
{
	"111"
	{
		"AccountName"		"first_login"
		"PersonaName"		"First Shown"
	}
	"222"
	{
		"AccountName"		"second_login"
		"PersonaName"		"Second Shown"
	}
}"#;
        assert_eq!(steam_persona(users, Some("SECOND_login")).as_deref(), Some("Second Shown"));
        assert_eq!(steam_persona(users, None), None, "several accounts and no hint: do not guess");
        let recent = users.replace("\"PersonaName\"\t\t\"First Shown\"", "\"PersonaName\"\t\t\"First Shown\"\n\t\t\"MostRecent\"\t\t\"1\"");
        assert_eq!(steam_persona(&recent, None).as_deref(), Some("First Shown"));
        let single = "\"users\"\n{\n\t\"9\"\n\t{\n\t\t\"AccountName\"\t\"only\"\n\t\t\"PersonaName\"\t\"Only One\"\n\t}\n}";
        assert_eq!(steam_persona(single, Some("someone_else")).as_deref(), Some("Only One"));
    }

    #[cfg(windows)]
    #[test]
    fn detects_running_and_absent_processes() {
        let me = std::env::current_exe().unwrap();
        let name = me.file_name().unwrap().to_string_lossy().into_owned();
        assert!(process_running(&name), "test binary {name} should be seen");
        assert!(process_running(&name.to_uppercase()));
        assert!(!process_running("no-such-process-ultimia-0.exe"));
    }
}
