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
use tauri_plugin_notification::NotificationExt;

use ocr4code::engine::tesseract;
use ocr4code::pipeline::EngineMode;
use ocr4code::render::{self, Format};

use crate::engine;
use crate::remote;
use crate::AppState;

#[cfg(target_os = "windows")]
mod win;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OverlayInfo {
    pub image: String,
}

#[cfg(target_os = "macos")]
#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    fn CGPreflightScreenCaptureAccess() -> u8;
    fn CGRequestScreenCaptureAccess() -> u8;
}

/// macOS asks for Screen Recording once per app (and only after a restart
/// takes effect). Windows captures through GDI, so there is no permission.
pub fn screen_access_status() -> &'static str {
    #[cfg(target_os = "macos")]
    {
        if screen_recording_granted() {
            "granted"
        } else {
            "denied"
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        "not-needed"
    }
}

pub fn request_screen_access() -> bool {
    #[cfg(target_os = "macos")]
    {
        unsafe { CGRequestScreenCaptureAccess() != 0 }
    }
    #[cfg(not(target_os = "macos"))]
    {
        true
    }
}

pub fn open_screen_settings() -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg("x-apple.systempreferences:com.apple.preference.security?Privacy_ScreenCapture")
            .status()
            .map_err(|e| format!("无法打开系统设置：{e}"))?;
        Ok(())
    }
    #[cfg(not(target_os = "macos"))]
    {
        Err("屏幕录制权限仅在 macOS 上需要。".into())
    }
}

#[cfg(target_os = "macos")]
fn screen_recording_granted() -> bool {
    unsafe { CGPreflightScreenCaptureAccess() != 0 }
}

#[cfg(not(target_os = "macos"))]
fn screen_recording_granted() -> bool {
    true
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
            let permission = raw.contains("E_PERMISSION") || raw.contains("屏幕录制权限");
            let blank = raw.contains("E_BLANK");
            let message = raw
                .replace("E_SETUP_REQUIRED: ", "")
                .replace("E_SETUP_REQUIRED", "")
                .replace("E_PERMISSION: ", "")
                .replace("E_PERMISSION", "")
                .replace("E_BLANK: ", "")
                .replace("E_BLANK", "");
            let hint = hint_for(&raw);
            let message = localize_error(&message);
            let code = if setup_required {
                "setup-required"
            } else if permission {
                "permission"
            } else if blank {
                "blank"
            } else if raw.to_lowercase().contains("llama")
                || raw.to_lowercase().contains("tesseract")
                || raw.contains("OCR")
                || raw.contains("识别")
                || raw.contains("运行时")
            {
                "engine"
            } else {
                "capture"
            };
            let _ = app.emit(
                "ocr:error",
                OcrFailure {
                    message,
                    hint,
                    code: Some(code.to_string()),
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

    #[cfg(target_os = "macos")]
    if !screen_recording_granted() {
        anyhow::bail!("E_PERMISSION: macOS 还没有授予屏幕录制权限");
    }

    let path = capture_path(app)?;
    let _ = std::fs::remove_file(&path);
    let cancel = &app.state::<AppState>().cancel;

    let _ = app.emit("ocr:phase", Phase { phase: "selecting" });
    if let CaptureOutcome::Cancelled = interactive_capture(app, &path, cancel)? {
        let _ = std::fs::remove_file(&path);
        let _ = app.emit("ocr:phase", Phase { phase: "idle" });
        return Ok(());
    }

    let inspection = inspect_capture(&path);
    if inspection.blank {
        let _ = std::fs::remove_file(&path);
        anyhow::bail!("E_BLANK: 截图区域几乎是空白的");
    }

    let mut config = settings.config();

    if settings.engine != EngineMode::Tesseract {
        let _ = app.emit("ocr:phase", Phase { phase: "starting" });
        let manager = app.state::<Arc<engine::Manager>>().inner().clone();
        let remote_manager = app.state::<Arc<remote::Manager>>().inner().clone();
        let remote_cfg = settings.remote_config().ok().flatten();
        let mut remote_ok = false;
        let mut remote_error: Option<String> = None;

        if let Some(cfg) = remote_cfg {
            match remote_manager.ensure_ready(&cfg) {
                Ok(url) => {
                    config.llama_url = Some(url);
                    remote_ok = true;
                }
                Err(remote::Error::Cancelled) => {
                    let _ = app.emit("ocr:phase", Phase { phase: "idle" });
                    return Ok(());
                }
                Err(err) => {
                    // A remote-only setup must surface the reason; `auto` still
                    // gets the local server and then tesseract as a fallback.
                    if settings.engine == EngineMode::LlamaCpp {
                        if matches!(err, remote::Error::NotDeployed(_)) {
                            return Err(anyhow::anyhow!("E_SETUP_REQUIRED: {err}"));
                        }
                        return Err(anyhow::anyhow!("{err}"));
                    }
                    remote_error = Some(err.to_string());
                }
            }
        }

        if !remote_ok {
            match manager.ensure_ready(&settings.llama_url, settings.server_config()) {
                Ok(url) => config.llama_url = Some(url),
                Err(engine::Error::Cancelled) => {
                    let _ = app.emit("ocr:phase", Phase { phase: "idle" });
                    return Ok(());
                }
                Err(err) => {
                    let fallback =
                        settings.engine == EngineMode::Auto && tesseract::available();
                    if fallback {
                        eprintln!("capture: engine unavailable ({err}); using tesseract");
                        config.engine = EngineMode::Tesseract;
                    } else if let Some(remote_error) = remote_error {
                        return Err(anyhow::anyhow!("{remote_error}"));
                    } else if matches!(err, engine::Error::SetupRequired) {
                        return Err(anyhow::anyhow!(
                            "E_SETUP_REQUIRED: OCR 模型还没有下载"
                        ));
                    } else {
                        return Err(anyhow::anyhow!("{err}"));
                    }
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
        thumbnail: inspection.thumbnail,
        captured_at,
        copied,
    };

    let _ = std::fs::remove_file(&path);
    let _ = app.emit("ocr:result", result.clone());
    let _ = app.emit("ocr:phase", Phase { phase: "idle" });
    notify_result(app, settings.notify_on_result, &result);

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

/// System notification for a finished capture. Deliberately only when the
/// window is not focused: with the app in front, the result panel is already
/// the feedback, and the UI guidelines say not to add a success toast.
fn notify_result(app: &AppHandle, enabled: bool, result: &OcrResult) {
    if !enabled {
        return;
    }
    let focused = app
        .get_webview_window("main")
        .and_then(|window| window.is_focused().ok())
        .unwrap_or(false);
    if focused {
        return;
    }
    if let Err(err) = app
        .notification()
        .builder()
        .title("识别完成")
        .body(notification_body(&result.text, result.lines, result.copied))
        .show()
    {
        eprintln!("capture: notification failed: {err}");
    }
}

/// First non-empty line, whitespace collapsed, capped at 80 characters, plus a
/// short status line. Kept separate so the wording stays testable.
fn notification_body(text: &str, lines: usize, copied: bool) -> String {
    let first = text
        .lines()
        .map(|line| line.split_whitespace().collect::<Vec<_>>().join(" "))
        .find(|line| !line.is_empty())
        .unwrap_or_default();
    let mut preview: String = first.chars().take(80).collect();
    if first.chars().count() > 80 {
        preview.push('…');
    }
    let action = if copied {
        "已复制到剪贴板"
    } else {
        "打开应用查看"
    };
    if preview.is_empty() {
        format!("{lines} 行 · {action}")
    } else {
        format!("{preview}\n{lines} 行 · {action}")
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
        "没有可用的 OCR 引擎。请下载 OCR 模型，或安装 tesseract。".into()
    } else if lower.contains("not reachable") {
        "连不上本地 OCR 引擎。请检查服务地址，或重新启动。".into()
    } else if lower.contains("tesseract failed") {
        "tesseract 运行失败。请检查安装和语言包。".into()
    } else {
        message.to_string()
    }
}

fn hint_for(message: &str) -> String {
    if message.contains("E_SETUP_REQUIRED") {
        return "下载 OCR 模型后即可截图。".into();
    }
    if message.contains("E_PERMISSION") || message.contains("屏幕录制权限") {
        return "在系统设置 → 隐私与安全性 → 屏幕录制中勾选 ocr4code，然后重启应用。"
            .into();
    }
    if message.contains("远程") || message.contains("服务器") {
        return "到设置 → OCR → 部署到服务器里检查连接，或重新部署。".into();
    }
    if message.contains("E_BLANK") {
        #[cfg(target_os = "macos")]
        return "如果框选区域本来就有文字，请检查屏幕录制权限，然后重启应用。"
            .into();
        #[cfg(not(target_os = "macos"))]
        return "请重新框选包含文字的区域。".into();
    }
    let lower = message.to_lowercase();
    if lower.contains("no engine available") || lower.contains("starting the ocr engine") {
        "先下载 OCR 模型，或安装 tesseract 后重试。".into()
    } else if lower.contains("not reachable") || lower.contains("llama") {
        "确认服务正在运行，并检查设置里的地址。".into()
    } else if lower.contains("permission") || lower.contains("screen") {
        "在系统设置 → 隐私与安全性 → 屏幕录制中勾选 ocr4code，然后重启应用。"
            .into()
    } else {
        "检查设置后重试。".into()
    }
}

struct CaptureInspection {
    blank: bool,
    thumbnail: Option<String>,
}

/// A capture made without screen-recording permission is an almost uniformly
/// colored frame. Decode once to check that and to build the result thumbnail.
fn inspect_capture(path: &Path) -> CaptureInspection {
    let Ok(image) = image::open(path) else {
        return CaptureInspection {
            blank: false,
            thumbnail: None,
        };
    };
    let thumb = image.thumbnail(520, 360);
    if is_uniform(&thumb) {
        return CaptureInspection {
            blank: true,
            thumbnail: None,
        };
    }
    let mut bytes = Cursor::new(Vec::new());
    let thumbnail = thumb
        .write_to(&mut bytes, image::ImageFormat::Png)
        .ok()
        .map(|()| {
            format!(
                "data:image/png;base64,{}",
                STANDARD.encode(bytes.into_inner())
            )
        });
    CaptureInspection {
        blank: false,
        thumbnail,
    }
}

/// Only a frame with no meaningful luminance range counts as blank. A real
/// selection containing text keeps its contrast after downscaling, so sparse
/// text is never mistaken for an empty region.
fn is_uniform(image: &image::DynamicImage) -> bool {
    const UNIFORM_LUMA_RANGE: i16 = 8;
    let luma = image.to_luma8();
    if luma.is_empty() {
        return true;
    }
    let mut min = i16::MAX;
    let mut max = i16::MIN;
    for pixel in luma.pixels() {
        let value = i16::from(pixel[0]);
        min = min.min(value);
        max = max.max(value);
        if max - min > UNIFORM_LUMA_RANGE {
            return false;
        }
    }
    true
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

#[cfg(test)]
mod tests {
    use super::*;

    fn image_with_line(width: u32, height: u32, top: u32) -> image::DynamicImage {
        let mut frame = image::RgbImage::from_pixel(width, height, image::Rgb([255, 255, 255]));
        for y in top..top + 20 {
            for x in 100..900 {
                frame.put_pixel(x, y, image::Rgb([20, 20, 20]));
            }
        }
        image::DynamicImage::ImageRgb8(frame)
    }

    #[test]
    fn a_uniform_frame_is_blank() {
        let frame = image::DynamicImage::ImageRgb8(image::RgbImage::from_pixel(
            3000,
            2000,
            image::Rgb([32, 32, 32]),
        ));
        assert!(is_uniform(&frame));
    }

    #[test]
    fn a_single_line_of_text_is_not_blank() {
        // One 800x20 line on a 4000x2800 frame is sparse by pixel count, but it
        // must survive downscaling and keep the frame from looking empty.
        assert!(!is_uniform(&image_with_line(4000, 2800, 1400)));
        assert!(!is_uniform(&image_with_line(6016, 3384, 1700)));
    }

    #[test]
    fn faint_gradient_counts_as_blank() {
        let mut frame = image::RgbImage::from_pixel(800, 600, image::Rgb([128, 128, 128]));
        for (_, y, pixel) in frame.enumerate_pixels_mut() {
            if y == 599 {
                *pixel = image::Rgb([133, 133, 133]);
            }
        }
        assert!(is_uniform(&image::DynamicImage::ImageRgb8(frame)));
    }

    #[test]
    fn notification_preview_uses_the_first_non_empty_line() {
        let body = notification_body("\n\n    let   value = 1;   // x\nfn main() {}", 2, true);
        assert_eq!(body, "let value = 1; // x\n2 行 · 已复制到剪贴板");
    }

    #[test]
    fn notification_preview_is_capped_and_has_a_fallback() {
        let long = "x".repeat(120);
        let body = notification_body(&long, 1, false);
        assert!(body.starts_with(&format!("{}…", "x".repeat(80))));
        assert!(body.ends_with("1 行 · 打开应用查看"));

        let body = notification_body("  \n\t\n", 0, false);
        assert_eq!(body, "0 行 · 打开应用查看");
    }
}
