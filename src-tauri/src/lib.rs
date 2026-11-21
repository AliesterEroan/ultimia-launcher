mod launcher;
mod vfun;

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use launcher::{ClearReport, LaunchConfig, LaunchOutcome, LoginStatus, ServerCheck, StatusReport, EmulatorStatus};
use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_dialog::DialogExt;

const GAME_APPEAR_TIMEOUT: Duration = Duration::from_secs(60);
static WATCHING: AtomicBool = AtomicBool::new(false);

fn show_main(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

#[tauri::command]
fn watch_game(app: AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.hide();
    }
    if WATCHING.swap(true, Ordering::SeqCst) {
        return;
    }
    std::thread::spawn(move || {
        let started = Instant::now();
        let mut seen = launcher::process_running(launcher::GAME_PROCESS);
        while !seen && started.elapsed() < GAME_APPEAR_TIMEOUT {
            std::thread::sleep(Duration::from_secs(1));
            seen = launcher::process_running(launcher::GAME_PROCESS);
        }
        if seen {
            while launcher::process_running(launcher::GAME_PROCESS) {
                std::thread::sleep(Duration::from_secs(2));
            }
        }
        WATCHING.store(false, Ordering::SeqCst);
        show_main(&app);
        let _ = app.emit("game-watch", if seen { "exited" } else { "not-seen" });
    });
}

#[tauri::command]
fn quit_app(app: AppHandle) {
    app.exit(0);
}

// Sync commands run on the main thread; anything that waits on SoulLauncher must not.
async fn off_main<T: Send + 'static>(work: impl FnOnce() -> T + Send + 'static) -> Result<T, String> {
    tauri::async_runtime::spawn_blocking(work).await.map_err(|e| e.to_string())
}

#[tauri::command]
async fn detect_install() -> Result<StatusReport, String> {
    off_main(launcher::detect_install).await
}

#[tauri::command]
async fn launch(config: LaunchConfig) -> Result<LaunchOutcome, String> {
    off_main(move || launcher::launch(config)).await?
}

#[tauri::command]
fn login_status() -> LoginStatus {
    launcher::login_status()
}

#[tauri::command]
async fn emulator_ensure(emulator_dir: Option<String>) -> Result<EmulatorStatus, String> {
    off_main(move || launcher::emulator_ensure(emulator_dir.as_deref())).await
}

#[tauri::command]
fn emulator_address(emulator_dir: Option<String>) -> String {
    launcher::emulator_address(emulator_dir.as_deref())
}

#[tauri::command]
async fn check_server(address: String) -> Result<ServerCheck, String> {
    off_main(move || launcher::check_server(&address)).await
}

fn open_url(url: &str) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let mut cmd = std::process::Command::new("cmd");
        cmd.args(["/c", "start", "", url]);
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
        let _ = cmd.spawn();
    }
    #[cfg(target_os = "macos")]
    {
        let _ = std::process::Command::new("open").arg(url).spawn();
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let _ = std::process::Command::new("xdg-open").arg(url).spawn();
    }
}

#[tauri::command]
async fn open_dashboard(app: tauri::AppHandle, emulator_dir: Option<String>) -> Result<EmulatorStatus, String> {
    let status = off_main(move || launcher::dashboard_ensure(emulator_dir.as_deref())).await?;
    // The standalone app opens/focuses its own window. Only fall back to an
    // embedded webview when the exe is missing but a server already listens.
    if status.running && !status.spawned {
        let url = format!("http://127.0.0.1:{}", launcher::dashboard_port());
        if let Some(win) = app.get_webview_window("dashboard") {
            let _ = win.unminimize();
            let _ = win.set_focus();
        } else {
            match tauri::WebviewWindowBuilder::new(
                &app,
                "dashboard",
                tauri::WebviewUrl::External(url.parse().expect("dashboard url")),
            )
            .title("ULTIMIADASH")
            .inner_size(1440.0, 900.0)
            .min_inner_size(900.0, 600.0)
            .build()
            {
                Ok(_) => {}
                Err(_) => open_url(&url),
            }
        }
    }
    Ok(status)
}

#[tauri::command]
async fn vfun_sign_in(user_id: String, password: String) -> Result<String, String> {
    off_main(move || vfun::sign_in(&user_id, password)).await?
}

#[tauri::command]
async fn vfun_sign_out() -> Result<ClearReport, String> {
    off_main(move || vfun::sign_out().map(|removed| ClearReport { removed })).await?
}

#[tauri::command]
fn vfun_account() -> Option<String> {
    vfun::detected_account()
}

#[tauri::command]
async fn pick_folder(
    app: tauri::AppHandle,
    default_dir: Option<String>,
) -> Result<Option<String>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let mut dialog = app.dialog().file();
        if let Some(dir) = default_dir {
            dialog = dialog.set_directory(dir);
        }
        dialog
            .blocking_pick_folder()
            .map(|p| p.to_string())
    })
    .await
    .map_err(|e| e.to_string())
}

#[tauri::command]
async fn pick_launcher_file(app: tauri::AppHandle) -> Result<Option<String>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        app.dialog()
            .file()
            .add_filter("SoulLauncher", &["exe"])
            .blocking_pick_file()
            .map(|p| p.to_string())
    })
    .await
    .map_err(|e| e.to_string())
}

#[tauri::command]
fn client_version(install_dir: Option<String>) -> Option<String> {
    launcher::client_version(install_dir)
}

#[tauri::command]
fn app_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let show = MenuItem::with_id(app, "show", "Show launcher", true, None::<&str>)?;
            let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&show, &quit])?;
            let mut tray = TrayIconBuilder::with_id("main-tray")
                .tooltip("Ultimia Launcher")
                .menu(&menu)
                .show_menu_on_left_click(false)
                .on_menu_event(|app, event| match event.id.as_ref() {
                    "show" => show_main(app),
                    "quit" => app.exit(0),
                    _ => {}
                })
                .on_tray_icon_event(|tray, event| {
                    if let TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        ..
                    } = event
                    {
                        show_main(tray.app_handle());
                    }
                });
            if let Some(icon) = app.default_window_icon() {
                tray = tray.icon(icon.clone());
            }
            tray.build(app)?;
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            detect_install,
            launch,
            emulator_ensure,
            emulator_address,
            check_server,
            open_dashboard,
            login_status,
            vfun_sign_in,
            vfun_sign_out,
            vfun_account,
            pick_folder,
            pick_launcher_file,
            client_version,
            app_version,
            watch_game,
            quit_app
        ])
        .run(tauri::generate_context!())
        .expect("error while running Ultimia Launcher");
}