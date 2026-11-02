//! Capture flows: native region selection, dropped files and clipboard images.
//!
//! macOS keeps `screencapture -i`; Windows uses our own GDI capture plus an
//! always-on-top Tauri selection window, so the Snipping Tool is never invoked.
//! Every entry point ends in the same place: ensure the engine, run the kernel,
//! copy the result, tell the UI.
//!
//! The work is split by concern: [`input`] validates dropped files and clipboard
//! images and turns them into a job, [`flow`] prepares the engine, recognizes
//! and classifies errors. This module owns the entry points and the job flow.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

use sofer::pipeline::EngineMode;

use crate::settings::{AppState, Settings};
use crate::toast;

use self::flow::{
    emit_fast_preview, inspect_capture, prepare_engine, recognize_to_result, report_error,
    retry_max_tokens, spawn_warm_up, CaptureInspection,
};

pub use self::input::{handle_drag_enter, handle_drag_leave, handle_drop};

use self::input::{claim_busy, prepare_clipboard_image, validate_image_file};

mod flow;
mod input;

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

/// One recognised line's ink region, in original-image pixels.
///
/// Only non-blank rows carry one: a blank line rebuilt from band spacing has
/// nothing in the image to outline.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PreviewBox {
    /// Index into `OcrResult::text`'s line list.
    pub line: usize,
    pub x0: f32,
    pub y0: f32,
    pub x1: f32,
    pub y1: f32,
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
    /// The kernel's trace says the reply may have stopped short of the image.
    /// The UI keeps the result and offers a chunked re-run.
    pub truncated: bool,
    /// Regions the model actually recognised; empty when the engine supplied
    /// no geometry. The UI draws these over the thumbnail when the user turns
    /// the preview-annotation setting on.
    pub preview_boxes: Vec<PreviewBox>,
}

/// Emitted while the model is still generating; `text` is everything so far.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OcrPartial {
    pub text: String,
    pub ms: u64,
    /// True when this is the fast Apple Vision pass, not model tokens.
    pub preview: bool,
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

/// Where the pixels for one recognition job come from.
enum JobSource {
    /// Region selection through the shortcut, tray item or Capture button.
    Screen,
    /// A file the user dropped or picked; we never delete it.
    File(PathBuf),
    /// An image read out of the clipboard into our cache; deleted afterwards.
    Clipboard(PathBuf),
}

/// Entry point for the global shortcut, tray item and the "Capture" button.
///
/// Returns immediately. A second trigger while a recognition is running is
/// treated as cancel instead of stacking jobs.
pub fn start(app: AppHandle) {
    let state = app.state::<AppState>();
    if state.busy.swap(true, Ordering::SeqCst) {
        // The shortcut doubles as "cancel the selection that is already open".
        state.cancel.store(true, Ordering::SeqCst);
        return;
    }
    state.cancel.store(false, Ordering::SeqCst);

    std::thread::spawn(move || {
        let result = run_job(&app, JobSource::Screen);
        finish_job(&app, result);
    });
}

/// Re-run the last capture that came back possibly truncated.
///
/// The screenshot is kept in the cache directory until the next capture starts,
/// so this never needs a new selection. It shares the busy flag with `start`:
/// a retry while something is running is ignored rather than stacked.
pub fn retry(app: AppHandle) {
    let state = app.state::<AppState>();
    if state.busy.swap(true, Ordering::SeqCst) {
        return;
    }
    state.cancel.store(false, Ordering::SeqCst);

    std::thread::spawn(move || {
        let result = run_retry(&app);
        finish_job(&app, result);
    });
}

fn spawn_job(app: AppHandle, source: JobSource) {
    std::thread::spawn(move || {
        let result = run_job(&app, source);
        finish_job(&app, result);
    });
}

fn run_job(app: &AppHandle, source: JobSource) -> Result<()> {
    let result = match &source {
        JobSource::Screen => run_screen(app),
        JobSource::File(path) | JobSource::Clipboard(path) => run_image(app, path),
    };
    cleanup_source(&source);
    result
}

/// Only the clipboard copy is ours. A dropped file belongs to the user.
fn cleanup_source(source: &JobSource) {
    if let JobSource::Clipboard(path) = source {
        let _ = std::fs::remove_file(path);
    }
}

fn finish_job(app: &AppHandle, result: Result<()>) {
    if let Err(err) = result {
        report_error(app, &err);
        let _ = app.emit("ocr:phase", Phase { phase: "idle" });
        show_main(app);
    }
    app.state::<AppState>().busy.store(false, Ordering::SeqCst);
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

fn run_screen(app: &AppHandle) -> Result<()> {
    let settings = current_settings(app);

    #[cfg(target_os = "macos")]
    if !screen_recording_granted() {
        anyhow::bail!("E_PERMISSION: macOS 还没有授予屏幕录制权限");
    }

    // A retained screenshot only exists so a truncated result can be retried;
    // a new capture makes it obsolete.
    discard_retained_capture(app);

    let path = capture_path(app)?;
    let _ = std::fs::remove_file(&path);
    let cancel = &app.state::<AppState>().cancel;

    let _ = app.emit("ocr:phase", Phase { phase: "selecting" });
    // Wake an engine that may be sleeping, in parallel with the drag.
    spawn_warm_up(app);
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

    match process_capture(app, &settings, &path, inspection, false) {
        Ok(Some(result)) => {
            if result.truncated {
                if let Err(err) = retain_capture(app, &path) {
                    eprintln!("capture: retaining the screenshot failed: {err:#}");
                }
            } else {
                let _ = std::fs::remove_file(&path);
            }
            complete(app, &settings, result);
            Ok(())
        }
        Ok(None) => {
            let _ = std::fs::remove_file(&path);
            Ok(())
        }
        Err(err) => {
            let _ = std::fs::remove_file(&path);
            Err(err)
        }
    }
}

/// A dropped file or a clipboard image. Reads the same thumbnail the capture
/// path builds so the result panel looks identical no matter the entry point.
/// The source file is never deleted here; clipboard copies are cleaned up by
/// `cleanup_source`.
fn run_image(app: &AppHandle, path: &Path) -> Result<()> {
    let settings = current_settings(app);
    let inspection = inspect_capture(path);
    if inspection.blank {
        anyhow::bail!("E_BLANK: 截图区域几乎是空白的");
    }
    if let Some(result) = process_capture(app, &settings, path, inspection, false)? {
        complete(app, &settings, result);
    }
    Ok(())
}

/// Re-run the retained screenshot, forcing a chunked pass.
fn run_retry(app: &AppHandle) -> Result<()> {
    let settings = current_settings(app);
    let path = retained_capture_path(app)?;
    if !path.exists() {
        anyhow::bail!("E_CAPTURE: 上一次的截图已经不存在");
    }

    let inspection = inspect_capture(&path);
    if inspection.blank {
        let _ = std::fs::remove_file(&path);
        anyhow::bail!("E_BLANK: 截图区域几乎是空白的");
    }

    let Some(result) = process_capture(app, &settings, &path, inspection, true)? else {
        return Ok(());
    };
    // A complete result no longer needs the pixels; a still-truncated one stays
    // retryable until the next capture starts.
    if !result.truncated {
        let _ = std::fs::remove_file(&path);
    }
    complete(app, &settings, result);
    Ok(())
}

fn current_settings(app: &AppHandle) -> Settings {
    app.state::<AppState>().settings.lock().unwrap().clone()
}

/// Recognise the selected pixels. `Ok(None)` means the user cancelled engine
/// startup (no error, no result).
fn process_capture(
    app: &AppHandle,
    settings: &Settings,
    path: &Path,
    inspection: CaptureInspection,
    chunked: bool,
) -> Result<Option<OcrResult>> {
    let mut config = settings.config();
    if settings.engine != EngineMode::Tesseract {
        let _ = app.emit("ocr:phase", Phase { phase: "starting" });
        if !prepare_engine(app, settings, &mut config)? {
            let _ = app.emit("ocr:phase", Phase { phase: "idle" });
            return Ok(None);
        }
    }

    if chunked {
        // More, smaller tiles: halve the single-call output budget and force
        // tiling even if the user turned it off. The kernel's own forced-split
        // retry still applies on top of this.
        config.tile = true;
        config.max_tokens = retry_max_tokens(config.max_tokens);
    }

    let _ = app.emit(
        "ocr:phase",
        Phase {
            phase: "recognizing",
        },
    );
    if settings.fast_preview {
        let limiter = sofer::pipeline::limiter(config.jobs);
        emit_fast_preview(app, &config, path, &limiter);
    }
    recognize_to_result(app, settings, &config, path, inspection).map(Some)
}

fn complete(app: &AppHandle, settings: &Settings, result: OcrResult) {
    let _ = app.emit("ocr:result", result.clone());
    let _ = app.emit("ocr:phase", Phase { phase: "idle" });

    if result.copied {
        // Never steal focus from the editor the user wants to paste into. When
        // the main window is not in front, the toast is the confirmation that
        // the text is already on the clipboard.
        toast::show_for_background(app, result.lines, settings.notify_on_result);
    } else {
        // Nothing reached the clipboard, so show the result and its copy button.
        show_main(app);
    }
}

pub fn show_main(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

fn capture_dir(app: &AppHandle) -> Result<PathBuf> {
    let dir = app
        .path()
        .app_cache_dir()
        .unwrap_or_else(|_| std::env::temp_dir())
        .join("captures");
    std::fs::create_dir_all(&dir).with_context(|| format!("创建截图目录 {}", dir.display()))?;
    Ok(dir)
}

fn capture_path(app: &AppHandle) -> Result<PathBuf> {
    Ok(capture_dir(app)?.join(format!("capture-{}.png", now_ms())))
}

/// The screenshot kept for a chunked retry. A fixed name means one file at
/// most, and it is always cleared when the next capture starts.
fn retained_capture_path(app: &AppHandle) -> Result<PathBuf> {
    Ok(capture_dir(app)?.join("retry.png"))
}

fn retain_capture(app: &AppHandle, path: &Path) -> Result<()> {
    let target = retained_capture_path(app)?;
    let _ = std::fs::remove_file(&target);
    std::fs::rename(path, &target).with_context(|| format!("保留截图 {}", target.display()))
}

fn discard_retained_capture(app: &AppHandle) {
    if let Ok(path) = retained_capture_path(app) {
        let _ = std::fs::remove_file(path);
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::ZERO)
        .as_millis() as u64
}

// ---------------------------------------------------------------------------
// Region selection
// ---------------------------------------------------------------------------

#[cfg(target_os = "macos")]
fn interactive_capture(
    _app: &AppHandle,
    dest: &Path,
    cancel: &AtomicBool,
) -> Result<CaptureOutcome> {
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
            return if status.success() && dest.exists() && std::fs::metadata(dest)?.len() > 0 {
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

/// A dropped file stays in the same size class as a large screenshot.
const MAX_IMAGE_BYTES: u64 = 40 * 1024 * 1024;

/// Extensions the kernel's decoder is built for (`image` crate features).
const SUPPORTED_IMAGE_EXTENSIONS: [&str; 6] = ["png", "jpg", "jpeg", "webp", "bmp", "tiff"];

/// Rejection of a dropped/clipboard image that never reached the kernel. The
/// UI shows it as a toast: what happened, then the one thing to change.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InputError {
    pub message: String,
    pub hint: String,
}

impl InputError {
    fn new(message: impl Into<String>, hint: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            hint: hint.into(),
        }
    }
}

/// Emitted while a file drag hovers over the window so the page can show a
/// drop target. `accepted` only looks at extensions; the size check needs the
/// file itself and runs on drop.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DragPayload {
    pub active: bool,
    pub accepted: bool,
}

/// Entry point for a file dropped onto the window. The path is validated here,
/// before any work starts, so the UI can show a concrete next step.
pub fn start_image_file(app: AppHandle, path: PathBuf) -> Result<(), InputError> {
    validate_image_file(&path)?;
    {
        let state = app.state::<AppState>();
        claim_busy(&state)?;
    }
    spawn_job(app, JobSource::File(path));
    Ok(())
}

/// Entry point for the "从剪贴板识别" button. The clipboard is read on the
/// calling thread so "no image" is reported as a normal command error.
pub fn start_clipboard_image(app: AppHandle) -> Result<(), InputError> {
    {
        let state = app.state::<AppState>();
        claim_busy(&state)?;
    }
    match prepare_clipboard_image(&app) {
        Ok(path) => {
            spawn_job(app, JobSource::Clipboard(path));
            Ok(())
        }
        Err(err) => {
            app.state::<AppState>().busy.store(false, Ordering::SeqCst);
            Err(err)
        }
    }
}

pub fn is_supported_image_path(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .map(|extension| {
            let extension = extension.to_ascii_lowercase();
            SUPPORTED_IMAGE_EXTENSIONS.contains(&extension.as_str())
        })
        .unwrap_or(false)
}

// ---------------------------------------------------------------------------
// Dropped files and clipboard images
// ---------------------------------------------------------------------------

#[tauri::command]
pub fn ocr_image_file(app: AppHandle, path: String) -> Result<(), InputError> {
    start_image_file(app, PathBuf::from(path))
}

#[tauri::command]
pub fn ocr_clipboard_image(app: AppHandle) -> Result<(), InputError> {
    start_clipboard_image(app)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_decodable_image_extensions_are_accepted() {
        for name in [
            "shot.png",
            "shot.JPG",
            "shot.jpeg",
            "shot.webp",
            "shot.bmp",
            "shot.tiff",
        ] {
            assert!(is_supported_image_path(Path::new(name)), "{name}");
        }
        for name in ["notes.txt", "anim.gif", "shot", "shot.png.exe", "shot.heic"] {
            assert!(!is_supported_image_path(Path::new(name)), "{name}");
        }
    }

    fn temp_file(tag: &str, contents: &[u8]) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path =
            std::env::temp_dir().join(format!("sofer-{tag}-{}-{unique}.png", std::process::id()));
        std::fs::write(&path, contents).unwrap();
        path
    }

    #[test]
    fn a_file_dropped_by_the_user_is_never_deleted() {
        let path = temp_file("keep", b"data");
        cleanup_source(&JobSource::File(path.clone()));
        assert!(path.exists());
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn the_clipboard_temp_file_is_removed_after_a_job() {
        let path = temp_file("clipboard", b"data");
        cleanup_source(&JobSource::Clipboard(path.clone()));
        assert!(!path.exists());
    }
}
