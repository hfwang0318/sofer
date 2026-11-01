//! Tauri shell around the OCR kernel.
//!
//! The webview owns presentation and settings drafting; this crate owns the
//! privileged parts: the global shortcut, screen selection, the kernel run, the
//! clipboard, the menu-bar item, the native window material, and the bundled
//! llama.cpp runtime with its one-click model bootstrap.

mod capture;
pub mod engine;
mod history;
pub mod remote;
mod settings;
mod toast;
mod window_style;

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::Serialize;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, DragDropEvent, Emitter, Manager, State, WindowEvent};
use tauri_plugin_clipboard_manager::ClipboardExt;
use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};

use sofer::engine::tesseract;
use sofer::pipeline::EngineMode;

use engine::{DownloadProgress, Event as EngineEvent, EventSink, Manager as EngineManager, Paths};
use settings::{AppState, Settings};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct AppInfo {
    shortcut_active: bool,
    shortcut: String,
    version: String,
}

#[tauri::command]
fn load_settings(state: State<'_, AppState>) -> Settings {
    state.settings.lock().unwrap().clone()
}

#[tauri::command]
fn save_settings(
    app: AppHandle,
    state: State<'_, AppState>,
    settings: Settings,
) -> Result<Settings, String> {
    settings::validate(&settings)?;

    let old = state.settings.lock().unwrap().clone();
    let shortcuts = app.global_shortcut();
    shortcuts.unregister_all().map_err(|e| e.to_string())?;
    if let Err(err) = shortcuts.register(settings.shortcut.as_str()) {
        let _ = shortcuts.register(old.shortcut.as_str());
        return Err(format!("无法注册快捷键“{}”：{err}", settings.shortcut));
    }
    if let Err(err) = settings::save(&app, &settings) {
        let _ = shortcuts.unregister_all();
        let _ = shortcuts.register(old.shortcut.as_str());
        return Err(err);
    }

    state.shortcut_active.store(true, Ordering::SeqCst);
    let quality_changed = old.model_quality != settings.model_quality;
    let restart = old.server_signature() != settings.server_signature();
    let remote_changed = old.remote_signature() != settings.remote_signature();
    *state.settings.lock().unwrap() = settings.clone();
    if restart {
        // `-c`/`-np`/KV/sleep/quality are baked into the child process. Stop it
        // so the next capture or the setup dialog starts the new plan.
        let manager = app.state::<Arc<EngineManager>>().inner().clone();
        if quality_changed {
            // An in-flight download is for the old pack.
            manager.cancel.store(true, Ordering::SeqCst);
        }
        manager.stop();
    }
    let remote_manager = app.state::<Arc<remote::Manager>>().inner().clone();
    remote_manager.set_enabled(settings.remote_enabled);
    if remote_changed {
        // The tunnel is derived from these fields. Drop it without an SSH
        // round trip; the next start/redeploy stops any process left behind
        // through the pid file, so saving Settings stays instant.
        remote_manager.cancel.store(true, Ordering::SeqCst);
        remote_manager.stop();
        if let Ok(Some(cfg)) = settings.remote_config() {
            // Refresh `deployed` (the quality or directory may have changed)
            // without blocking the save.
            let manager = remote_manager.clone();
            std::thread::spawn(move || {
                if let Err(err) = manager.probe(&cfg) {
                    eprintln!("remote: probe after settings change failed: {err}");
                }
            });
        }
    }
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.set_zoom(settings.ui_scale);
        // AppKit calls must stay on the main thread; commands run on the pool.
        let scale = settings.ui_scale;
        let _ = app.run_on_main_thread(move || window_style::round_native_corners(&window, scale));
    }
    Ok(settings)
}

/// The global shortcut would otherwise fire while the user is recording it in
/// Settings. The recorder suspends it on focus and resumes on blur/cancel.
#[tauri::command]
fn set_shortcut_enabled(
    app: AppHandle,
    state: State<'_, AppState>,
    enabled: bool,
) -> Result<bool, String> {
    let shortcuts = app.global_shortcut();
    if enabled {
        shortcuts.unregister_all().map_err(|e| e.to_string())?;
        let shortcut = state.settings.lock().unwrap().shortcut.clone();
        match shortcuts.register(shortcut.as_str()) {
            Ok(()) => {
                state.shortcut_active.store(true, Ordering::SeqCst);
                Ok(true)
            }
            Err(err) => {
                state.shortcut_active.store(false, Ordering::SeqCst);
                Err(format!("无法重新注册“{shortcut}”：{err}"))
            }
        }
    } else {
        shortcuts.unregister_all().map_err(|e| e.to_string())?;
        Ok(false)
    }
}

#[tauri::command]
fn app_info(state: State<'_, AppState>) -> AppInfo {
    AppInfo {
        shortcut_active: state.shortcut_active.load(Ordering::SeqCst),
        shortcut: state.settings.lock().unwrap().shortcut.clone(),
        version: env!("CARGO_PKG_VERSION").to_string(),
    }
}

#[tauri::command]
fn copy_text(app: AppHandle, text: String) -> Result<(), String> {
    app.clipboard()
        .write_text(text)
        .map_err(|e| format!("写入剪贴板失败：{e}"))
}

#[tauri::command]
fn capture_now(app: AppHandle) {
    capture::start(app);
}

/// Re-run the retained screenshot with a chunked pass. The result arrives on
/// the same `ocr:*` events as a normal capture.
#[tauri::command]
fn retry_capture(app: AppHandle) {
    capture::retry(app);
}

#[tauri::command]
fn quit_app(app: AppHandle) {
    app.exit(0);
}

#[tauri::command]
fn restart_app(app: AppHandle) {
    app.restart();
}

#[tauri::command]
fn screen_access() -> String {
    capture::screen_access_status().to_string()
}

#[tauri::command]
fn request_screen_access() -> bool {
    capture::request_screen_access()
}

#[tauri::command]
fn open_screen_settings() -> Result<(), String> {
    capture::open_screen_settings()
}

#[tauri::command]
async fn probe_engine(url: String) -> Result<bool, String> {
    let url = url.trim().to_string();
    if !(url.starts_with("http://") || url.starts_with("https://")) {
        return Err("请输入完整的 http:// 或 https:// 地址。".into());
    }
    tauri::async_runtime::spawn_blocking(move || {
        engine::health_ok(&url, Duration::from_millis(1200))
    })
    .await
    .map_err(|e| format!("检测失败：{e}"))
}

// ---------------------------------------------------------------------------
// Engine / model lifecycle
// ---------------------------------------------------------------------------

fn runtime_dir(app: &AppHandle) -> PathBuf {
    if let Ok(resource) = app.path().resource_dir() {
        let candidate = resource.join("runtime").join("llama");
        if engine::runtime_ready_at(&candidate) {
            return candidate;
        }
    }
    // Development fallback: `pnpm tauri dev` may not have copied resources yet.
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("runtime/macos-arm64")
    }
    #[cfg(all(target_os = "macos", not(target_arch = "aarch64")))]
    {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("runtime/macos-x64")
    }
    #[cfg(target_os = "windows")]
    {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("runtime/windows-x64")
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("runtime")
    }
}

fn engine_paths(app: &AppHandle) -> Paths {
    let models = app
        .path()
        .app_data_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .join("models")
        .join("glm-ocr");
    let logs = app
        .path()
        .app_log_dir()
        .unwrap_or_else(|_| models.join("logs"));
    Paths {
        runtime: runtime_dir(app),
        models,
        logs,
    }
}

fn engine_sink(app: AppHandle) -> EventSink {
    Arc::new(move |event| match event {
        EngineEvent::Download(progress) => {
            let _ = app.emit("engine:download", progress);
        }
        EngineEvent::State(state) => {
            let _ = app.emit("engine:state", state);
        }
    })
}

fn current_engine_url(app: &AppHandle) -> String {
    app.state::<AppState>()
        .settings
        .lock()
        .unwrap()
        .llama_url
        .clone()
}

fn emit_download_phase(app: &AppHandle, phase: &str, message: Option<&str>) {
    let quality = app
        .state::<AppState>()
        .settings
        .lock()
        .unwrap()
        .model_quality;
    let total = engine::model_total(quality);
    let payload = DownloadProgress {
        phase: phase.to_string(),
        file: None,
        downloaded: if phase == "ready" { total } else { 0 },
        total,
        bytes_per_sec: 0,
        eta_secs: None,
        message: message.map(str::to_string),
    };
    let _ = app.emit("engine:download", payload);
}

/// The status the React layer sees: the local kernel status plus the remote
/// deployment state. `flatten` keeps the existing field names stable.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct DesktopStatus {
    #[serde(flatten)]
    engine: engine::Status,
    remote: remote::State,
}

fn remote_sink(app: AppHandle) -> remote::EventSink {
    Arc::new(move |event| match event {
        remote::Event::Progress(progress) => {
            let _ = app.emit("remote:progress", progress);
        }
        remote::Event::State(state) => {
            let _ = app.emit("remote:state", state);
        }
    })
}

fn remote_config(app: &AppHandle) -> Result<remote::RemoteConfig, String> {
    let settings = app.state::<AppState>().settings.lock().unwrap().clone();
    settings
        .remote_config()?
        .ok_or_else(|| "先在设置 → OCR 中启用远程部署。".to_string())
}

fn merge_remote(mut status: engine::Status, enabled: bool, remote: remote::State) -> DesktopStatus {
    if enabled {
        if remote.phase == "online" {
            status.llama = true;
            if let Some(url) = remote.url.clone() {
                status.llama_url = url;
            }
        }
        if remote.deployed {
            // The local copy is irrelevant when the server has the model.
            status.setup.needed = false;
        }
    }
    DesktopStatus {
        engine: status,
        remote,
    }
}

/// Starts or refreshes the configured engine. Used by autostart and the
/// Settings "start" button.
#[tauri::command]
async fn start_engine(app: AppHandle) -> Result<DesktopStatus, String> {
    let manager = app.state::<Arc<EngineManager>>().inner().clone();
    let remote_manager = app.state::<Arc<remote::Manager>>().inner().clone();
    let settings = app.state::<AppState>().settings.lock().unwrap().clone();
    let url = settings.llama_url.clone();
    let config = settings.server_config();
    let remote_cfg = settings.remote_config()?;
    let result = tauri::async_runtime::spawn_blocking(move || {
        match &remote_cfg {
            Some(cfg) => {
                remote_manager
                    .ensure_ready(cfg)
                    .map_err(|e| e.to_string())?;
            }
            None => {
                manager
                    .ensure_ready(&url, config)
                    .map_err(|e| e.to_string())?;
            }
        }
        let mut status = manager.status(&url, config.quality);
        status.tesseract = tesseract::available();
        Ok::<_, String>((status, remote_manager.state()))
    })
    .await
    .map_err(|e| format!("无法启动 OCR 引擎：{e}"))??;
    Ok(merge_remote(result.0, settings.remote_enabled, result.1))
}

#[tauri::command]
async fn stop_engine(app: AppHandle) -> Result<(), String> {
    let manager = app.state::<Arc<EngineManager>>().inner().clone();
    let remote_manager = app.state::<Arc<remote::Manager>>().inner().clone();
    let settings = app.state::<AppState>().settings.lock().unwrap().clone();
    let remote_cfg = settings.remote_config().ok().flatten();
    tauri::async_runtime::spawn_blocking(move || {
        remote_manager.cancel.store(true, Ordering::SeqCst);
        manager.stop();
        match remote_cfg {
            Some(cfg) => remote_manager.stop_with(&cfg),
            None => remote_manager.stop(),
        }
    })
    .await
    .map_err(|e| format!("无法停止 OCR 引擎：{e}"))?;
    Ok(())
}

#[tauri::command]
async fn prepare_update(app: AppHandle) -> Result<(), String> {
    let manager = app.state::<Arc<EngineManager>>().inner().clone();
    let remote_manager = app.state::<Arc<remote::Manager>>().inner().clone();
    let remote_cfg = app
        .state::<AppState>()
        .settings
        .lock()
        .unwrap()
        .remote_config()
        .ok()
        .flatten();
    tauri::async_runtime::spawn_blocking(move || {
        manager.prepare_update();
        if let Some(cfg) = remote_cfg {
            remote_manager.stop_with(&cfg);
        }
    })
    .await
    .map_err(|e| format!("无法停止 OCR 引擎：{e}"))?;
    Ok(())
}

#[tauri::command]
async fn remove_model(app: AppHandle) -> Result<(), String> {
    let manager = app.state::<Arc<EngineManager>>().inner().clone();
    let quality = app
        .state::<AppState>()
        .settings
        .lock()
        .unwrap()
        .model_quality;
    tauri::async_runtime::spawn_blocking(move || {
        manager.remove_model(quality).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| format!("移除模型失败：{e}"))?
}

/// One-click setup: download both files (resuming), verify SHA-256, start the
/// bundled server. Progress is emitted on `engine:download` / `engine:state`.
#[tauri::command]
fn setup_engine(app: AppHandle) -> Result<(), String> {
    let manager = app.state::<Arc<EngineManager>>().inner().clone();
    let url = current_engine_url(&app);
    let settings = app.state::<AppState>().settings.lock().unwrap().clone();
    let options = settings.download_options()?;
    let server = settings.server_config();
    if manager.setup_running.swap(true, Ordering::SeqCst) {
        return Ok(());
    }
    manager.reset_cancel();

    std::thread::spawn(move || {
        match manager.bootstrap(&url, &options, server) {
            Ok(()) => {}
            Err(engine::Error::Cancelled) => {
                emit_download_phase(
                    &app,
                    "cancelled",
                    Some("下载已取消，已下载的部分会保留，可继续。"),
                );
            }
            Err(err) => {
                emit_download_phase(&app, "error", Some(&err.to_string()));
            }
        }
        manager.setup_running.store(false, Ordering::SeqCst);
    });
    Ok(())
}

#[tauri::command]
fn cancel_engine_setup(app: AppHandle) {
    app.state::<Arc<EngineManager>>()
        .cancel
        .store(true, Ordering::SeqCst);
}

#[tauri::command]
async fn engine_status(app: AppHandle) -> Result<DesktopStatus, String> {
    let manager = app.state::<Arc<EngineManager>>().inner().clone();
    let remote_manager = app.state::<Arc<remote::Manager>>().inner().clone();
    let settings = app.state::<AppState>().settings.lock().unwrap().clone();
    let url = settings.llama_url.clone();
    let quality = settings.model_quality;
    remote_manager.set_enabled(settings.remote_enabled);
    let (mut status, remote_state) = tauri::async_runtime::spawn_blocking(move || {
        let mut status = manager.status(&url, quality);
        status.tesseract = tesseract::available();
        (status, remote_manager.state())
    })
    .await
    .map_err(|e| format!("无法检查 OCR 引擎状态：{e}"))?;
    status.tesseract = tesseract::available();
    Ok(merge_remote(status, settings.remote_enabled, remote_state))
}

// ---------------------------------------------------------------------------
// Remote deployment
// ---------------------------------------------------------------------------

/// Inspect the SSH target: OS, arch, tools, existing weights and free disk.
#[tauri::command]
async fn remote_probe(app: AppHandle) -> Result<remote::Probe, String> {
    let cfg = remote_config(&app)?;
    let manager = app.state::<Arc<remote::Manager>>().inner().clone();
    tauri::async_runtime::spawn_blocking(move || manager.probe(&cfg).map_err(|e| e.to_string()))
        .await
        .map_err(|e| format!("检测服务器失败：{e}"))?
}

/// One-click deploy. Returns immediately; progress arrives on `remote:*`.
#[tauri::command]
fn remote_deploy(app: AppHandle, source: Option<remote::Source>) -> Result<(), String> {
    let mut settings = app.state::<AppState>().settings.lock().unwrap().clone();
    if let Some(source) = source {
        settings.remote_source = source;
    }
    let cfg = settings
        .remote_config()?
        .ok_or_else(|| "先在设置 → OCR 中启用远程部署。".to_string())?;
    let manager = app.state::<Arc<remote::Manager>>().inner().clone();
    if manager.deploy_running.swap(true, Ordering::SeqCst) {
        return Ok(());
    }
    manager.cancel.store(false, Ordering::SeqCst);
    std::thread::spawn(move || {
        if let Err(err) = manager.deploy(&cfg, cfg.source) {
            eprintln!("remote: deploy failed: {err}");
        }
        manager.deploy_running.store(false, Ordering::SeqCst);
    });
    Ok(())
}

#[tauri::command]
async fn remote_start(app: AppHandle) -> Result<DesktopStatus, String> {
    let manager = app.state::<Arc<EngineManager>>().inner().clone();
    let remote_manager = app.state::<Arc<remote::Manager>>().inner().clone();
    let settings = app.state::<AppState>().settings.lock().unwrap().clone();
    let cfg = settings
        .remote_config()?
        .ok_or_else(|| "先在设置 → OCR 中启用远程部署。".to_string())?;
    let url = settings.llama_url.clone();
    let quality = settings.model_quality;
    let (mut status, remote_state) = tauri::async_runtime::spawn_blocking(move || {
        remote_manager
            .ensure_ready(&cfg)
            .map_err(|e| e.to_string())?;
        let mut status = manager.status(&url, quality);
        status.tesseract = tesseract::available();
        Ok::<_, String>((status, remote_manager.state()))
    })
    .await
    .map_err(|e| format!("无法启动远程 OCR 引擎：{e}"))??;
    status.tesseract = tesseract::available();
    Ok(merge_remote(status, settings.remote_enabled, remote_state))
}

#[tauri::command]
async fn remote_stop(app: AppHandle) -> Result<DesktopStatus, String> {
    let manager = app.state::<Arc<EngineManager>>().inner().clone();
    let remote_manager = app.state::<Arc<remote::Manager>>().inner().clone();
    let settings = app.state::<AppState>().settings.lock().unwrap().clone();
    let cfg = settings.remote_config().ok().flatten();
    let url = settings.llama_url.clone();
    let quality = settings.model_quality;
    let (mut status, remote_state) = tauri::async_runtime::spawn_blocking(move || {
        remote_manager.cancel.store(true, Ordering::SeqCst);
        match cfg {
            Some(cfg) => remote_manager.stop_with(&cfg),
            None => remote_manager.stop(),
        }
        let mut status = manager.status(&url, quality);
        status.tesseract = tesseract::available();
        (status, remote_manager.state())
    })
    .await
    .map_err(|e| format!("无法停止远程 OCR 引擎：{e}"))?;
    status.tesseract = tesseract::available();
    Ok(merge_remote(status, settings.remote_enabled, remote_state))
}

#[tauri::command]
fn remote_cancel(app: AppHandle) {
    app.state::<Arc<remote::Manager>>()
        .cancel
        .store(true, Ordering::SeqCst);
}

fn autostart_engine(app: &AppHandle) {
    let settings = app.state::<AppState>().settings.lock().unwrap().clone();
    if settings.engine == EngineMode::Tesseract {
        return;
    }
    if let Ok(Some(cfg)) = settings.remote_config() {
        let remote_manager = app.state::<Arc<remote::Manager>>().inner().clone();
        if !settings.remote_auto_start {
            // Still inspect the server once so the UI knows whether the model
            // is already deployed, without starting or connecting anything.
            std::thread::spawn(move || {
                if let Err(err) = remote_manager.probe(&cfg) {
                    eprintln!("remote: startup probe failed: {err}");
                }
            });
            return;
        }
        std::thread::spawn(move || {
            if let Err(err) = remote_manager.ensure_ready(&cfg) {
                eprintln!("remote: autostart failed: {err}");
            }
        });
        return;
    }
    let manager = app.state::<Arc<EngineManager>>().inner().clone();
    let config = settings.server_config();
    if !manager.model_info(config.quality).installed {
        return;
    }
    std::thread::spawn(move || {
        if let Err(err) = manager.ensure_ready(&settings.llama_url, config) {
            eprintln!("engine: autostart failed: {err}");
        }
    });
}

// ---------------------------------------------------------------------------
// Tray / window
// ---------------------------------------------------------------------------

fn build_tray(app: &AppHandle) -> tauri::Result<()> {
    let capture_item = MenuItem::with_id(app, "capture", "截图", true, None::<&str>)?;
    let show_item = MenuItem::with_id(app, "show", "打开 sofer", true, None::<&str>)?;
    let separator = PredefinedMenuItem::separator(app)?;
    let quit_item = MenuItem::with_id(app, "quit", "退出 sofer", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&capture_item, &show_item, &separator, &quit_item])?;

    let mut builder = TrayIconBuilder::with_id("main-tray")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .tooltip("sofer — 截图转文字")
        .on_menu_event(|app, event| match event.id.as_ref() {
            "capture" => capture::start(app.clone()),
            "show" => capture::show_main(app),
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
                capture::show_main(tray.app_handle());
            }
        });

    #[cfg(target_os = "macos")]
    {
        // A template icon lets macOS invert it for light/dark menu bars.
        let icon = tauri::image::Image::from_bytes(include_bytes!("../icons/trayTemplate@2x.png"))?;
        builder = builder.icon(icon).icon_as_template(true);
    }
    #[cfg(not(target_os = "macos"))]
    {
        // Windows/Linux do not template tray icons; the white glyph would vanish
        // on a light taskbar, so reuse the colour app icon instead.
        if let Some(icon) = app.default_window_icon() {
            builder = builder.icon(icon.clone());
        }
    }

    builder.build(app)?;
    Ok(())
}

pub fn run() {
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            capture::show_main(app);
        }))
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(|app, _shortcut, event| {
                    if event.state() == ShortcutState::Pressed {
                        capture::start(app.clone());
                    }
                })
                .build(),
        )
        .plugin(tauri_plugin_clipboard_manager::init())
        .invoke_handler(tauri::generate_handler![
            load_settings,
            save_settings,
            history::load_history,
            history::save_history,
            history::migrate_history,
            app_info,
            set_shortcut_enabled,
            copy_text,
            capture_now,
            retry_capture,
            restart_app,
            screen_access,
            request_screen_access,
            open_screen_settings,
            probe_engine,
            capture::overlay_info,
            capture::overlay_finish,
            capture::overlay_cancel,
            capture::ocr_image_file,
            capture::ocr_clipboard_image,
            toast::toast_info,
            toast::toast_dismiss,
            engine_status,
            setup_engine,
            cancel_engine_setup,
            prepare_update,
            start_engine,
            stop_engine,
            remove_model,
            remote_probe,
            remote_deploy,
            remote_start,
            remote_stop,
            remote_cancel,
            quit_app
        ])
        .setup(|app| {
            let handle = app.handle().clone();
            let loaded = settings::load(&handle);
            app.manage(AppState {
                settings: Mutex::new(loaded.clone()),
                busy: AtomicBool::new(false),
                cancel: AtomicBool::new(false),
                shortcut_active: AtomicBool::new(false),
            });

            let paths = engine_paths(&handle);
            let local_models = paths.models.clone();
            let manager = Arc::new(EngineManager::new(paths, engine_sink(handle.clone())));
            app.manage(manager.clone());

            let remote_manager = Arc::new(remote::Manager::new(
                local_models,
                remote_sink(handle.clone()),
            ));
            remote_manager.set_enabled(loaded.remote_enabled);
            app.manage(remote_manager.clone());

            // Tauri covers normal Quit; this covers SIGTERM/SIGINT (logout,
            // terminal Ctrl-C) so the managed server is not orphaned.
            #[cfg(unix)]
            {
                use signal_hook::consts::{SIGINT, SIGTERM};
                use signal_hook::iterator::Signals;
                match Signals::new([SIGINT, SIGTERM]) {
                    Ok(mut signals) => {
                        let signal_manager = manager.clone();
                        let signal_remote = remote_manager.clone();
                        std::thread::spawn(move || {
                            if signals.forever().next().is_some() {
                                signal_manager.stop();
                                signal_remote.shutdown();
                                std::process::exit(0);
                            }
                        });
                    }
                    Err(err) => eprintln!("signal handler: {err}"),
                }
            }

            if let Some(window) = app.get_webview_window("main") {
                #[cfg(target_os = "macos")]
                {
                    use window_vibrancy::{
                        apply_vibrancy, NSVisualEffectMaterial, NSVisualEffectState,
                    };
                    if let Err(err) = apply_vibrancy(
                        &window,
                        NSVisualEffectMaterial::HudWindow,
                        Some(NSVisualEffectState::Active),
                        Some(16.0),
                    ) {
                        eprintln!("window material: {err}");
                    }
                }
                #[cfg(target_os = "windows")]
                {
                    use window_vibrancy::apply_acrylic;
                    if let Err(err) = apply_acrylic(&window, Some((16, 17, 22, 170))) {
                        eprintln!("window material: {err}");
                    }
                }
                let zoom = loaded.ui_scale.clamp(1.0, 1.3);
                let _ = window.set_zoom(zoom);
                window_style::round_native_corners(&window, zoom);
                window.show()?;
            }

            let shortcut_active = match app.global_shortcut().register(loaded.shortcut.as_str()) {
                Ok(()) => true,
                Err(err) => {
                    eprintln!(
                        "global shortcut “{}” was not registered: {err}",
                        loaded.shortcut
                    );
                    false
                }
            };
            app.state::<AppState>()
                .shortcut_active
                .store(shortcut_active, Ordering::SeqCst);

            build_tray(&handle)?;
            autostart_engine(&handle);
            Ok(())
        })
        .on_window_event(|window, event| {
            if window.label() != "main" {
                return;
            }
            match event {
                // Closing the control window parks the app in the menu bar so
                // the global shortcut keeps working. Quit lives in the tray menu.
                WindowEvent::CloseRequested { api, .. } => {
                    api.prevent_close();
                    let _ = window.hide();
                }
                // The native drag handler suppresses the webview's HTML5 drop
                // events, so file drops are reported from here. The page then
                // calls `ocr_image_file`; both entry points validate the same
                // way and share the busy flag.
                WindowEvent::DragDrop(DragDropEvent::Enter { paths, .. }) => {
                    capture::handle_drag_enter(window, paths);
                }
                WindowEvent::DragDrop(DragDropEvent::Leave) => {
                    capture::handle_drag_leave(window);
                }
                WindowEvent::DragDrop(DragDropEvent::Drop { paths, .. }) => {
                    capture::handle_drop(window, paths.clone());
                }
                _ => {}
            }
        })
        .build(tauri::generate_context!())
        .expect("error while building sofer");

    app.run(|app_handle, event| {
        if let tauri::RunEvent::Exit = event {
            if let Some(manager) = app_handle.try_state::<Arc<EngineManager>>() {
                manager.stop();
            }
            if let Some(remote_manager) = app_handle.try_state::<Arc<remote::Manager>>() {
                remote_manager.shutdown();
            }
        }
    });
}
