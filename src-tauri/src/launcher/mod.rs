use serde::{Deserialize, Serialize};

mod detect;
mod launch;
mod server;

pub use detect::{client_version, detect_install, game_dirs, process_running, resolve_launcher_path};
pub use launch::{launch, login_status, validate_server_address};
pub use server::{check_server, dashboard_ensure, dashboard_port, emulator_address, emulator_ensure};

const CREATE_NO_WINDOW: u32 = 0x0800_0000;

pub const GAME_PROCESS: &str = "SoulWorker.exe";

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LaunchConfig {
    pub server: String,
    pub install_source: String,
    pub custom_folder: Option<String>,
    #[serde(default)]
    pub skip_version_check: bool,
    pub launcher_path: Option<String>,
    #[serde(default)]
    pub official_servers: OfficialServers,
    /// Local-emulator launches use the ID/password arg path
    /// (`VFunQA:: MultiExecuteClient`), which the client handles correctly;
    /// the `fromVLauncher::` token hand-off leaves the server-select scene
    /// empty on 2.3.34.7.
    #[serde(default)]
    pub local_emulator: bool,
}

/// Official server addresses sent in the Global hand-off. Defaults are the ones
/// the official launcher ships with; Settings → Advanced may override them.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct OfficialServers {
    pub global1: String,
    pub global2: String,
    pub china: String,
}

impl Default for OfficialServers {
    fn default() -> Self {
        Self {
            global1: "43.159.147.134:10000".into(),
            global2: "43.130.7.93:10000".into(),
            china: "49.233.245.180:10000".into(),
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StatusReport {
    pub launcher_path: Option<String>,
    pub found: bool,
    pub server: Option<String>,
    pub install: Option<String>,
    pub error: Option<String>,
    /// Every install found, each "SOURCE - path" (VFUN and Steam can coexist).
    #[serde(default)]
    pub installs: Vec<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LaunchOutcome {
    pub args: Vec<String>,
    pub exit_code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
    pub timed_out: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LoginStatus {
    pub steam_name: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClearReport {
    pub removed: Vec<String>,
}

/// Reachability probe result returned by `check_server`.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerCheck {
    pub reachable: bool,
    pub latency_ms: Option<u64>,
    pub port: Option<u16>,
    pub error: Option<String>,
    /// True when the verdict came from (or was corrected by) Ultimiadash.
    pub via_dashboard: bool,
}

/// Ultimiadash /api/status snapshot — the dashboard probes the configured
/// bind IP, so it stays correct when the server isn't listening on loopback.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DashboardStatus {
    pub login: bool,
    pub game: bool,
    pub ip: String,
    pub login_port: u16,
    pub game_port: u16,
}

/// Local server (emulator) status returned by `emulator_ensure`.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EmulatorStatus {
    pub running: bool,
    pub spawned: bool,
    pub note: String,
}
