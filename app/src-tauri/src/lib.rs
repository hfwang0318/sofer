//! Tauri shell around the OCR kernel.
//!
//! The webview owns presentation and settings drafting; this crate owns the
//! privileged parts: the global shortcut, screen selection, the kernel run, the
//! clipboard, the menu-bar item, the native window material, and the bundled
//! llama.cpp runtime with its one-click model bootstrap.

mod capture;
pub mod engine;
mod settings;

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use serde::Serialize;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager, State, WindowEvent};
use tauri_plugin_clipboard_manager::ClipboardExt;
use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};

use ocr4code::engine::tesseract;
use ocr4code::pipeline::EngineMode;

use engine::{DownloadProgress, Event as EngineEvent, EventSink, Manager as EngineManager, Paths};
use settings::{AppState, Settings};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct AppInfo {
    shortcut_active: bool,
    shortcut: String,
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
        return Err(format!(
            "无法注册快捷键“{}”：{err}",
            settings.shortcut
        ));
    }
    if let Err(err) = settings::save(&app, &settings) {
        let _ = shortcuts.unregister_all();
        let _ = shortcuts.register(old.shortcut.as_str());
        return Err(err);
    }

    state.shortcut_active.store(true, Ordering::SeqCst);
    *state.settings.lock().unwrap() = settings.clone();
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

#[tauri::command]
fn quit_app(app: AppHandle) {
    app.exit(0);
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
    let total = engine::MODEL_TOTAL_BYTES;
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

/// Starts or refreshes the local engine. Used by autostart and the Settings
/// "Start engine" button. Returns the full status so the caller can update.
#[tauri::command]
async fn start_engine(app: AppHandle) -> Result<engine::Status, String> {
    let manager = app.state::<Arc<EngineManager>>().inner().clone();
    let url = current_engine_url(&app);
    tauri::async_runtime::spawn_blocking(move || {
        manager.ensure_ready(&url).map_err(|e| e.to_string())?;
        let mut status = manager.status(&url);
        status.tesseract = tesseract::available();
        Ok::<_, String>(status)
    })
    .await
    .map_err(|e| format!("启动引擎失败：{e}"))?
}

#[tauri::command]
fn stop_engine(app: AppHandle) {
    app.state::<Arc<EngineManager>>().stop();
}

#[tauri::command]
async fn remove_model(app: AppHandle) -> Result<(), String> {
    let manager = app.state::<Arc<EngineManager>>().inner().clone();
    tauri::async_runtime::spawn_blocking(move || manager.remove_model().map_err(|e| e.to_string()))
        .await
        .map_err(|e| format!("移除模型失败：{e}"))?
}

/// One-click setup: download both files (resuming), verify SHA-256, start the
/// bundled server. Progress is emitted on `engine:download` / `engine:state`.
#[tauri::command]
fn setup_engine(app: AppHandle) -> Result<(), String> {
    let manager = app.state::<Arc<EngineManager>>().inner().clone();
    if manager.setup_running.swap(true, Ordering::SeqCst) {
        return Ok(());
    }
    manager.reset_cancel();
    let url = current_engine_url(&app);
    let settings = app.state::<AppState>().settings.lock().unwrap().clone();
    let options = engine::DownloadOptions {
        model_base: Some(settings.model_base),
        proxy: Some(settings.download_proxy),
    };

    std::thread::spawn(move || {
        match manager.bootstrap(&url, &options) {
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
async fn engine_status(app: AppHandle) -> Result<engine::Status, String> {
    let manager = app.state::<Arc<EngineManager>>().inner().clone();
    let url = current_engine_url(&app);
    tauri::async_runtime::spawn_blocking(move || {
        let mut status = manager.status(&url);
        status.tesseract = tesseract::available();
        status
    })
    .await
    .map_err(|e| format!("引擎检测失败：{e}"))
}

fn autostart_engine(app: &AppHandle) {
    let manager = app.state::<Arc<EngineManager>>().inner().clone();
    let settings = app.state::<AppState>().settings.lock().unwrap().clone();
    if settings.engine == EngineMode::Tesseract {
        return;
    }
    if !manager.model_info().installed {
        return;
    }
    std::thread::spawn(move || {
        if let Err(err) = manager.ensure_ready(&settings.llama_url) {
            eprintln!("engine: autostart failed: {err}");
        }
    });
}

// ---------------------------------------------------------------------------
// Tray / window
// ---------------------------------------------------------------------------

fn build_tray(app: &AppHandle) -> tauri::Result<()> {
    let capture_item = MenuItem::with_id(app, "capture", "截取区域", true, None::<&str>)?;
    let show_item = MenuItem::with_id(app, "show", "打开 ocr4code", true, None::<&str>)?;
    let separator = PredefinedMenuItem::separator(app)?;
    let quit_item = MenuItem::with_id(app, "quit", "退出 ocr4code", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&capture_item, &show_item, &separator, &quit_item])?;

    let mut builder = TrayIconBuilder::with_id("main-tray")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .tooltip("ocr4code — 截图转文字")
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
            app_info,
            set_shortcut_enabled,
            copy_text,
            capture_now,
            capture::overlay_info,
            capture::overlay_finish,
            capture::overlay_cancel,
            engine_status,
            setup_engine,
            cancel_engine_setup,
            start_engine,
            stop_engine,
            remove_model,
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

            let manager = Arc::new(EngineManager::new(
                engine_paths(&handle),
                engine_sink(handle.clone()),
            ));
            app.manage(manager.clone());

            // Tauri covers normal Quit; this covers SIGTERM/SIGINT (logout,
            // terminal Ctrl-C) so the managed server is not orphaned.
            #[cfg(unix)]
            {
                use signal_hook::consts::{SIGINT, SIGTERM};
                use signal_hook::iterator::Signals;
                match Signals::new([SIGINT, SIGTERM]) {
                    Ok(mut signals) => {
                        let signal_manager = manager.clone();
                        std::thread::spawn(move || {
                            if signals.forever().next().is_some() {
                                signal_manager.stop();
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
            // Closing the control window parks the app in the menu bar so the
            // global shortcut keeps working. Quit lives in the tray menu.
            if let WindowEvent::CloseRequested { api, .. } = event {
                if window.label() == "main" {
                    api.prevent_close();
                    let _ = window.hide();
                }
            }
        })
        .build(tauri::generate_context!())
        .expect("error while building ocr4code");

    app.run(|app_handle, event| {
        if let tauri::RunEvent::Exit = event {
            if let Some(manager) = app_handle.try_state::<Arc<EngineManager>>() {
                manager.stop();
            }
        }
    });
}
