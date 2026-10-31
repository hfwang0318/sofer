//! Global-shortcut capture flow: native region selection → kernel → clipboard.
//!
//! macOS keeps `screencapture -i`; Windows uses our own GDI capture plus an
//! always-on-top Tauri selection window, so the Snipping Tool is never invoked.
//! Everything after the pixels are chosen is ours: run the kernel, copy the
//! result, tell the UI.

use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use base64::{engine::general_purpose::STANDARD, Engine as _};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_clipboard_manager::ClipboardExt;

use ocr4code::engine::tesseract;
use ocr4code::pipeline::EngineMode;
use ocr4code::render::{self, Format};

use crate::engine;
use crate::AppState;

#[cfg(target_os = "windows")]
mod win;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OverlayInfo {
    pub image: String,
}

/// A successful recognition, shaped for the React side.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OcrResult {
    pub id: String,
    pub text: String,
    pub engine: String,
    pub ms: u64,
    pub width: u32,
    pub height: u32,
    pub language: Option<String>,
    pub format: String,
    pub chars: usize,
    pub lines: usize,
    pub thumbnail: Option<String>,
    pub captured_at: u64,
    pub copied: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct OcrFailure {
    message: String,
    hint: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    code: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct Phase {
    phase: &'static str,
}

enum CaptureOutcome {
    Selected,
    Cancelled,
}

/// Entry point for the global shortcut, tray item and the "Capture" button.
///
/// Returns immediately. A second trigger while a selection is open is ignored
/// instead of stacking overlays.
pub fn start(app: AppHandle) {
    let state = app.state::<AppState>();
    if state.busy.swap(true, Ordering::SeqCst) {
        // The shortcut doubles as "cancel the selection that is already open".
        state.cancel.store(true, Ordering::SeqCst);
        return;
    }
    state.cancel.store(false, Ordering::SeqCst);

    std::thread::spawn(move || {
        let result = run(&app);
        if let Err(err) = result {
            let raw = format!("{err:#}");
            eprintln!("capture: {raw}");
            let setup_required = raw.contains("E_SETUP_REQUIRED");
            let message = raw
                .replace("E_SETUP_REQUIRED: ", "")
                .replace("E_SETUP_REQUIRED", "");
            let hint = hint_for(&raw, setup_required);
            let message = localize_error(&message);
            let _ = app.emit(
                "ocr:error",
                OcrFailure {
                    message,
                    hint,
                    code: setup_required.then(|| "setup-required".to_string()),
                },
            );
            let _ = app.emit("ocr:phase", Phase { phase: "idle" });
            show_main(&app);
        }
        app.state::<AppState>().busy.store(false, Ordering::SeqCst);
    });
}

#[tauri::command]
pub fn overlay_info() -> Result<OverlayInfo, String> {
    #[cfg(target_os = "windows")]
    {
        win::overlay_info()
    }
    #[cfg(not(target_os = "windows"))]
    {
        Err("截图浮层仅支持 Windows".into())
    }
}

#[tauri::command]
pub fn overlay_finish(
    window: tauri::WebviewWindow,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
    scale: f64,
) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        win::overlay_finish(window, x, y, width, height, scale)
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = (window, x, y, width, height, scale);
        Err("截图浮层仅支持 Windows".into())
    }
}

#[tauri::command]
pub fn overlay_cancel() -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        win::overlay_cancel()
    }
    #[cfg(not(target_os = "windows"))]
    {
        Ok(())
    }
}

fn run(app: &AppHandle) -> Result<()> {
    let settings = app
        .state::<AppState>()
        .settings
        .lock()
        .unwrap()
        .clone();
    let main_was_visible = app
        .get_webview_window("main")
        .and_then(|w| w.is_visible().ok())
        .unwrap_or(true);

    let path = capture_path(app)?;
    let _ = std::fs::remove_file(&path);
    let cancel = &app.state::<AppState>().cancel;

    let _ = app.emit("ocr:phase", Phase { phase: "selecting" });
    if let CaptureOutcome::Cancelled = interactive_capture(app, &path, cancel)? {
        let _ = std::fs::remove_file(&path);
        let _ = app.emit("ocr:phase", Phase { phase: "idle" });
        return Ok(());
    }

    let mut config = settings.config();

    if settings.engine != EngineMode::Tesseract {
        let _ = app.emit("ocr:phase", Phase { phase: "starting" });
        let manager = app.state::<Arc<engine::Manager>>().inner().clone();
        match manager.ensure_ready(&settings.llama_url, settings.server_config()) {
            Ok(url) => config.llama_url = Some(url),
            Err(engine::Error::Cancelled) => {
                let _ = app.emit("ocr:phase", Phase { phase: "idle" });
                return Ok(());
            }
            Err(err) => {
                let fallback = settings.engine == EngineMode::Auto && tesseract::available();
                if fallback {
                    eprintln!("capture: local engine unavailable ({err}); using tesseract");
                    config.engine = EngineMode::Tesseract;
                } else if matches!(err, engine::Error::SetupRequired) {
                    return Err(anyhow::anyhow!(
                        "E_SETUP_REQUIRED: 本地 OCR 模型尚未安装"
                    ));
                } else {
                    return Err(anyhow::anyhow!("{err}"));
                }
            }
        }
    }

    let _ = app.emit("ocr:phase", Phase { phase: "recognizing" });
    let engine = config.engine().context("启动 OCR 引擎")?;
    let limiter = ocr4code::pipeline::limiter(config.jobs);
    let outcome = ocr4code::pipeline::recognize(engine.as_ref(), &path, &config, &limiter)?;
    let text = render::render(&outcome.document, config.format, config.language.as_deref());

    let copied = settings.auto_copy
        && app
            .clipboard()
            .write_text(text.clone())
            .map(|_| true)
            .unwrap_or(false);

    let captured_at = now_ms();
    let chars = text.chars().count();
    let result = OcrResult {
        id: format!("{captured_at}-{chars}"),
        text,
        engine: outcome.document.engine.clone(),
        ms: outcome.document.ms,
        width: outcome.document.width,
        height: outcome.document.height,
        language: outcome.document.language.clone(),
        format: format_name(config.format),
        chars,
        lines: outcome.document.lines.len(),
        thumbnail: thumbnail(&path),
        captured_at,
        copied,
    };

    let _ = std::fs::remove_file(&path);
    let _ = app.emit("ocr:result", result.clone());
    let _ = app.emit("ocr:phase", Phase { phase: "idle" });

    if result.copied {
        // Never steal focus from the editor the user wants to paste into. If the
        // control window is hidden, the tray tooltip is the only confirmation.
        if !main_was_visible {
            if let Some(tray) = app.tray_by_id("main-tray") {
                let _ = tray.set_tooltip(Some(format!(
                    "ocr4code — 已复制 {} 行",
                    result.lines
                )));
            }
        }
    } else {
        // Nothing reached the clipboard, so show the result and its copy button.
        show_main(app);
    }
    Ok(())
}

pub fn show_main(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

fn capture_path(app: &AppHandle) -> Result<PathBuf> {
    let dir = app
        .path()
        .app_cache_dir()
        .unwrap_or_else(|_| std::env::temp_dir())
        .join("captures");
    std::fs::create_dir_all(&dir)
        .with_context(|| format!("创建截图目录 {}", dir.display()))?;
    Ok(dir.join(format!("capture-{}.png", now_ms())))
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::ZERO)
        .as_millis() as u64
}

fn format_name(format: Format) -> String {
    match format {
        Format::Text => "text",
        Format::Markdown => "markdown",
        Format::Json => "json",
    }
    .to_string()
}

fn localize_error(message: &str) -> String {
    let lower = message.to_lowercase();
    if lower.contains("no engine available") {
        "没有可用的 OCR 引擎。请启动 llama-server，或安装 tesseract。".into()
    } else if lower.contains("not reachable") {
        "无法连接 llama.cpp 服务。请确认服务已经启动，并检查设置中的地址。".into()
    } else if lower.contains("tesseract failed") {
        "Tesseract 运行失败。请检查安装和语言包。".into()
    } else {
        message.to_string()
    }
}

fn hint_for(message: &str, setup_required: bool) -> String {
    if setup_required {
        return "本地模型需要一次性下载。打开引导设置即可启用 OCR，无需手动安装。".into();
    }
    let lower = message.to_lowercase();
    if lower.contains("no engine available") || lower.contains("starting the ocr engine") {
        "请启动 llama-server，或安装 tesseract 作为兜底，然后在设置中检测引擎。"
            .into()
    } else if lower.contains("not reachable") || lower.contains("llama") {
        "请确认 llama-server 正在运行，并检查设置中的地址是否正确。"
            .into()
    } else if lower.contains("permission") || lower.contains("screen") {
        "请在系统设置 → 隐私与安全 → 屏幕录制中授予权限，然后重启应用。"
            .into()
    } else {
        "请打开设置检查引擎和版面选项，然后重试。".into()
    }
}

fn thumbnail(path: &Path) -> Option<String> {
    let image = image::open(path).ok()?;
    let thumb = image.thumbnail(520, 360);
    let mut bytes = Cursor::new(Vec::new());
    thumb
        .write_to(&mut bytes, image::ImageFormat::Png)
        .ok()?;
    Some(format!(
        "data:image/png;base64,{}",
        STANDARD.encode(bytes.into_inner())
    ))
}

// ---------------------------------------------------------------------------
// Region selection
// ---------------------------------------------------------------------------

#[cfg(target_os = "macos")]
fn interactive_capture(_app: &AppHandle, dest: &Path, cancel: &AtomicBool) -> Result<CaptureOutcome> {
    use std::process::Command;
    use std::time::Instant;

    let mut child = Command::new("/usr/sbin/screencapture")
        .args(["-i", "-x", "-o", "-t", "png"])
        .arg(dest)
        .spawn()
        .context("运行 screencapture 失败（是否已授予屏幕录制权限？）")?;

    let started = Instant::now();
    loop {
        if let Some(status) = child.try_wait()? {
            return if status.success()
                && dest.exists()
                && std::fs::metadata(dest)?.len() > 0
            {
                Ok(CaptureOutcome::Selected)
            } else {
                Ok(CaptureOutcome::Cancelled)
            };
        }
        if cancel.load(Ordering::Relaxed) || started.elapsed() > Duration::from_secs(300) {
            let _ = child.kill();
            let _ = child.wait();
            return Ok(CaptureOutcome::Cancelled);
        }
        std::thread::sleep(Duration::from_millis(80));
    }
}

#[cfg(target_os = "windows")]
fn interactive_capture(
    app: &AppHandle,
    dest: &Path,
    cancel: &AtomicBool,
) -> Result<CaptureOutcome> {
    win::interactive_capture(app, dest, cancel)
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn interactive_capture(
    _app: &AppHandle,
    _dest: &Path,
    _cancel: &AtomicBool,
) -> Result<CaptureOutcome> {
    anyhow::bail!("截图仅支持 macOS 和 Windows")
}
