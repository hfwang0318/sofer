//! Capture flows: native region selection, dropped files and clipboard images.
//!
//! macOS keeps `screencapture -i`; Windows uses our own GDI capture plus an
//! always-on-top Tauri selection window, so the Snipping Tool is never invoked.
//! Every entry point ends in the same place: ensure the engine, run the kernel,
//! copy the result, tell the UI.

use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use base64::{engine::general_purpose::STANDARD, Engine as _};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_clipboard_manager::ClipboardExt;
use tauri_plugin_notification::NotificationExt;

use ocr4code::engine::tesseract;
use ocr4code::pipeline::{Config, EngineMode};
use ocr4code::render::{self, Format};

use crate::engine;
use crate::remote;
use crate::settings::{AppState, Settings};

/// A dropped file stays in the same size class as a large screenshot.
const MAX_IMAGE_BYTES: u64 = 40 * 1024 * 1024;

/// Extensions the kernel's decoder is built for (`image` crate features).
const SUPPORTED_IMAGE_EXTENSIONS: [&str; 6] = ["png", "jpg", "jpeg", "webp", "bmp", "tiff"];

/// One token per IPC event would flood the webview; ~8 updates a second is
/// enough for a preview. The first delta is always sent immediately.
const PARTIAL_EMIT_INTERVAL: Duration = Duration::from_millis(120);

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
    /// The kernel's trace says the reply may have stopped short of the image.
    /// The UI keeps the result and offers a chunked re-run.
    pub truncated: bool,
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

/// Copy the clipboard image to a temp PNG. A failure here must release the
/// busy flag and leave no partial file behind.
fn prepare_clipboard_image(app: &AppHandle) -> Result<PathBuf, InputError> {
    let path = clipboard_path(app)?;
    if let Err(err) = write_clipboard_image(&path) {
        let _ = std::fs::remove_file(&path);
        return Err(err);
    }
    Ok(path)
}

/// Native drop delivered by the Tauri window event. The page decides what to
/// do with the paths; this only filters out the ones no engine could read.
pub fn handle_drop(window: &tauri::Window, mut paths: Vec<PathBuf>) {
    let accepted = paths.iter().any(|path| is_supported_image_path(path));
    let _ = window.emit(
        "ocr:drag",
        DragPayload {
            active: false,
            accepted,
        },
    );
    // The page starts with the first path; put a real image there when the
    // drop mixed files (a screenshot plus a stray README, say).
    paths.sort_by_key(|path| !is_supported_image_path(path));
    let paths: Vec<String> = paths
        .iter()
        .map(|path| path.display().to_string())
        .collect();
    let _ = window.emit("ocr:drop", paths);
}

/// The drop target shown while a drag hovers over the window.
pub fn handle_drag_enter(window: &tauri::Window, paths: &[PathBuf]) {
    let accepted = paths.iter().any(|path| is_supported_image_path(path));
    let _ = window.emit(
        "ocr:drag",
        DragPayload {
            active: true,
            accepted,
        },
    );
}

pub fn handle_drag_leave(window: &tauri::Window) {
    let _ = window.emit(
        "ocr:drag",
        DragPayload {
            active: false,
            accepted: true,
        },
    );
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

fn claim_busy(state: &AppState) -> Result<(), InputError> {
    if state.busy.swap(true, Ordering::SeqCst) {
        return Err(InputError::new(
            "正在识别上一张图",
            "等这次识别结束再试。",
        ));
    }
    state.cancel.store(false, Ordering::SeqCst);
    Ok(())
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

/// Same classification for every entry point: the capture page renders one
/// error panel and the shortcut path never shows a different vocabulary.
fn report_error(app: &AppHandle, err: &anyhow::Error) {
    let raw = format!("{err:#}");
    eprintln!("capture: {raw}");
    let setup_required = raw.contains("E_SETUP_REQUIRED");
    let permission = raw.contains("E_PERMISSION") || raw.contains("屏幕录制权限");
    let blank = raw.contains("E_BLANK");
    let capture_only = raw.contains("E_CAPTURE");
    let message = raw
        .replace("E_SETUP_REQUIRED: ", "")
        .replace("E_SETUP_REQUIRED", "")
        .replace("E_PERMISSION: ", "")
        .replace("E_PERMISSION", "")
        .replace("E_BLANK: ", "")
        .replace("E_BLANK", "")
        .replace("E_CAPTURE: ", "")
        .replace("E_CAPTURE", "");
    let hint = hint_for(&raw);
    let message = localize_error(&message);
    let code = if setup_required {
        "setup-required"
    } else if permission {
        "permission"
    } else if blank {
        "blank"
    } else if capture_only {
        "capture"
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
    let main_was_visible = main_window_visible(app);

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
            complete(app, &settings, main_was_visible, result);
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
    let main_was_visible = main_window_visible(app);
    let inspection = inspect_capture(path);
    if inspection.blank {
        anyhow::bail!("E_BLANK: 截图区域几乎是空白的");
    }
    if let Some(result) = process_capture(app, &settings, path, inspection, false)? {
        complete(app, &settings, main_was_visible, result);
    }
    Ok(())
}

/// Re-run the retained screenshot, forcing a chunked pass.
fn run_retry(app: &AppHandle) -> Result<()> {
    let settings = current_settings(app);
    let main_was_visible = main_window_visible(app);
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
    complete(app, &settings, main_was_visible, result);
    Ok(())
}

fn current_settings(app: &AppHandle) -> Settings {
    app.state::<AppState>().settings.lock().unwrap().clone()
}

fn main_window_visible(app: &AppHandle) -> bool {
    app.get_webview_window("main")
        .and_then(|window| window.is_visible().ok())
        .unwrap_or(true)
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

    let _ = app.emit("ocr:phase", Phase { phase: "recognizing" });
    if settings.fast_preview {
        let limiter = ocr4code::pipeline::limiter(config.jobs);
        emit_fast_preview(app, &config, path, &limiter);
    }
    recognize_to_result(app, settings, &config, path, inspection).map(Some)
}

fn retry_max_tokens(current: u32) -> u32 {
    (current / 2).max(1024)
}

/// Resolve the engine and pin the config to what actually answered.
///
/// `ensure_ready` already proved that the engine is up, so leaving the mode on
/// `auto` would health-check it a second time (800 ms) and a slow-but-healthy
/// server could silently fall back to tesseract. `auto` only falls back when
/// `ensure_ready` itself failed. Returns `Ok(false)` when the user cancelled.
fn prepare_engine(app: &AppHandle, settings: &Settings, config: &mut Config) -> Result<bool> {
    let manager = app.state::<Arc<engine::Manager>>().inner().clone();
    let remote_manager = app.state::<Arc<remote::Manager>>().inner().clone();
    let remote_cfg = settings.remote_config().ok().flatten();
    let mut remote_error: Option<String> = None;

    if let Some(cfg) = remote_cfg {
        match remote_manager.ensure_ready(&cfg) {
            Ok(url) => {
                use_llama(config, url);
                return Ok(true);
            }
            Err(remote::Error::Cancelled) => return Ok(false),
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

    match manager.ensure_ready(&settings.llama_url, settings.server_config()) {
        Ok(url) => {
            use_llama(config, url);
            Ok(true)
        }
        Err(engine::Error::Cancelled) => Ok(false),
        Err(err) => {
            if settings.engine == EngineMode::Auto && tesseract::available() {
                eprintln!("capture: engine unavailable ({err}); using tesseract");
                config.engine = EngineMode::Tesseract;
                // Never leave a stale address behind for the tesseract run.
                config.llama_url = None;
                Ok(true)
            } else if let Some(remote_error) = remote_error {
                Err(anyhow::anyhow!("{remote_error}"))
            } else if matches!(err, engine::Error::SetupRequired) {
                Err(anyhow::anyhow!("E_SETUP_REQUIRED: OCR 模型还没有下载"))
            } else {
                Err(anyhow::anyhow!("{err}"))
            }
        }
    }
}

fn use_llama(config: &mut Config, url: String) {
    config.engine = EngineMode::LlamaCpp;
    config.llama_url = Some(url);
}

/// Show a fast Apple Vision result before the local model finishes.
///
/// Best effort: only when the user opted in and the shim is available. The
/// preview is never copied, never added to history and never replaces the
/// final result; it only shortens the time until text appears.
fn emit_fast_preview(
    app: &AppHandle,
    config: &Config,
    path: &Path,
    limiter: &ocr4code::pool::Semaphore,
) {
    if !cfg!(target_os = "macos") || !ocr4code::engine::vision::available() {
        return;
    }
    let fast = fast_preview_config(config);
    let Ok(engine) = fast.engine() else {
        return;
    };
    let started = Instant::now();
    match ocr4code::pipeline::recognize(engine.as_ref(), path, &fast, limiter) {
        Ok(outcome) => {
            let text = render::render(&outcome.document, config.format, config.language.as_deref());
            let _ = app.emit(
                "ocr:partial",
                OcrPartial {
                    text,
                    ms: started.elapsed().as_millis() as u64,
                    preview: true,
                },
            );
        }
        Err(err) => eprintln!("capture: fast preview failed: {err:#}"),
    }
}

/// The fast pass always uses one whole image: Vision is fast enough that
/// tiling would add complexity without helping, and the final pass owns the
/// real tiling decision.
fn fast_preview_config(config: &Config) -> Config {
    let mut fast = config.clone();
    fast.engine = EngineMode::Vision;
    fast.tile = false;
    fast
}

fn recognize_to_result(
    app: &AppHandle,
    settings: &Settings,
    config: &Config,
    path: &Path,
    inspection: CaptureInspection,
) -> Result<OcrResult> {
    let engine = config.engine().context("启动 OCR 引擎")?;
    let limiter = ocr4code::pipeline::limiter(config.jobs);

    let started = Instant::now();
    let mut accumulated = String::new();
    let mut last_emit: Option<Instant> = None;
    let mut emitted_len = 0usize;
    let outcome = {
        let mut on_delta = |delta: &str| {
            accumulated.push_str(delta);
            let now = Instant::now();
            let due = last_emit
                .map(|last| now.duration_since(last) >= PARTIAL_EMIT_INTERVAL)
                .unwrap_or(true);
            if due {
                last_emit = Some(now);
                emitted_len = accumulated.len();
                let _ = app.emit(
                    "ocr:partial",
                    OcrPartial {
                        text: accumulated.clone(),
                        ms: started.elapsed().as_millis() as u64,
                        preview: false,
                    },
                );
            }
        };
        let outcome = ocr4code::pipeline::recognize_streaming(
            engine.as_ref(),
            path,
            config,
            &limiter,
            &mut on_delta,
        )?;
        // Ship the tail that arrived between the last throttle tick and the end.
        if emitted_len < accumulated.len() {
            let _ = app.emit(
                "ocr:partial",
                OcrPartial {
                    text: accumulated.clone(),
                    ms: started.elapsed().as_millis() as u64,
                    preview: false,
                },
            );
        }
        outcome
    };

    let text = render::render(&outcome.document, config.format, config.language.as_deref());

    let copied = settings.auto_copy
        && app
            .clipboard()
            .write_text(text.clone())
            .map(|_| true)
            .unwrap_or(false);

    let captured_at = now_ms();
    let chars = text.chars().count();
    Ok(OcrResult {
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
        truncated: !outcome.trace.consistent,
    })
}

fn complete(app: &AppHandle, settings: &Settings, main_was_visible: bool, result: OcrResult) {
    let _ = app.emit("ocr:result", result.clone());
    let _ = app.emit("ocr:phase", Phase { phase: "idle" });
    notify_result(app, settings.notify_on_result, &result);

    if result.copied {
        // Never steal focus from the editor the user wants to paste into. If the
        // control window is hidden, the tray tooltip is the only confirmation.
        if !main_was_visible {
            if let Some(tray) = app.tray_by_id("main-tray") {
                let _ = tray.set_tooltip(Some(format!("ocr4code — 已复制 {} 行", result.lines)));
            }
        }
    } else {
        // Nothing reached the clipboard, so show the result and its copy button.
        show_main(app);
    }
}

// ---------------------------------------------------------------------------
// Selection warm-up
// ---------------------------------------------------------------------------

/// Ask a ready engine for one token while the user is still dragging, so an
/// idle-sleeping server is awake by the time the screenshot arrives. Best
/// effort: it never blocks selection and never touches `busy` / `cancel`.
fn spawn_warm_up(app: &AppHandle) {
    let settings = current_settings(app);
    if settings.engine == EngineMode::Tesseract {
        return;
    }
    let manager = app.state::<Arc<engine::Manager>>().inner().clone();
    let remote_manager = app.state::<Arc<remote::Manager>>().inner().clone();
    std::thread::spawn(move || {
        let Some(url) = ready_engine_url(&settings, &manager, &remote_manager) else {
            return;
        };
        if !engine::warm_up(&url, Duration::from_secs(6)) {
            eprintln!("capture: warm-up request did not complete");
        }
    });
}

/// Only a server that is already up is worth waking; starting one is
/// `ensure_ready`'s job and would duplicate work.
fn ready_engine_url(
    settings: &Settings,
    manager: &engine::Manager,
    remote_manager: &remote::Manager,
) -> Option<String> {
    if settings.remote_enabled {
        let state = remote_manager.state();
        if state.phase == "online" {
            return state.url;
        }
        return None;
    }
    let status = manager.status(&settings.llama_url, settings.model_quality);
    if status.server.state == "online" {
        status.server.url
    } else {
        None
    }
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

fn validate_image_file(path: &Path) -> Result<(), InputError> {
    if !is_supported_image_path(path) {
        return Err(InputError::new(
            "不支持这个格式",
            "换成 PNG、JPG、WebP、BMP 或 TIFF 再试。",
        ));
    }
    let metadata = std::fs::metadata(path).map_err(|err| {
        eprintln!("capture: dropped file {}: {err}", path.display());
        InputError::new("找不到这个文件", "确认文件还在原位置，再拖一次。")
    })?;
    if !metadata.is_file() {
        return Err(InputError::new(
            "这不是一个文件",
            "拖入图片文件本身，不要拖文件夹。",
        ));
    }
    check_image_size(metadata.len())
}

fn check_image_size(bytes: u64) -> Result<(), InputError> {
    if bytes > MAX_IMAGE_BYTES {
        Err(InputError::new(
            "图片太大了",
            format!(
                "上限 {} MB，先压缩或裁剪再试。",
                MAX_IMAGE_BYTES / (1024 * 1024)
            ),
        ))
    } else {
        Ok(())
    }
}

/// Cache path unique per process, so two running copies never fight over the
/// same clipboard image.
fn clipboard_path(app: &AppHandle) -> Result<PathBuf, InputError> {
    let dir = app
        .path()
        .app_cache_dir()
        .unwrap_or_else(|_| std::env::temp_dir())
        .join("captures");
    std::fs::create_dir_all(&dir).map_err(|err| {
        eprintln!("capture: clipboard dir {}: {err}", dir.display());
        InputError::new("存不下这张图", "重启应用后再试。")
    })?;
    Ok(dir.join(format!(
        "clipboard-{}-{}.png",
        std::process::id(),
        now_ms()
    )))
}

fn write_clipboard_image(path: &Path) -> Result<(), InputError> {
    let mut clipboard = arboard::Clipboard::new().map_err(|err| {
        eprintln!("capture: clipboard unavailable: {err}");
        InputError::new("读不到剪贴板", "重启应用后再试。")
    })?;
    let image = match clipboard.get_image() {
        Ok(image) => image,
        Err(arboard::Error::ContentNotAvailable) => {
            return Err(InputError::new(
                "剪贴板里没有图片",
                "先截图或复制一张图片，再试一次。",
            ));
        }
        Err(err) => {
            eprintln!("capture: clipboard image failed: {err}");
            return Err(InputError::new(
                "剪贴板读不出图片",
                "复制一张图片后重试。",
            ));
        }
    };

    let width = u32::try_from(image.width).unwrap_or(0);
    let height = u32::try_from(image.height).unwrap_or(0);
    let buffer = image::RgbaImage::from_raw(width, height, image.bytes.into_owned())
        .filter(|buffer| !buffer.is_empty())
        .ok_or_else(|| InputError::new("剪贴板里的图片读不出来", "换一张图复制后重试。"))?;
    image::DynamicImage::ImageRgba8(buffer)
        .save_with_format(path, image::ImageFormat::Png)
        .map_err(|err| {
            eprintln!("capture: saving clipboard image: {err}");
            InputError::new("剪贴板图片存不下来", "重试一次；还不行就换一张图。")
        })?;

    let bytes = std::fs::metadata(path).map(|meta| meta.len()).unwrap_or(0);
    if let Err(err) = check_image_size(bytes) {
        let _ = std::fs::remove_file(path);
        return Err(err);
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

fn capture_dir(app: &AppHandle) -> Result<PathBuf> {
    let dir = app
        .path()
        .app_cache_dir()
        .unwrap_or_else(|_| std::env::temp_dir())
        .join("captures");
    std::fs::create_dir_all(&dir)
        .with_context(|| format!("创建截图目录 {}", dir.display()))?;
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
    std::fs::rename(path, &target)
        .with_context(|| format!("保留截图 {}", target.display()))
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

    fn test_state() -> AppState {
        AppState {
            settings: std::sync::Mutex::new(Settings::default()),
            busy: AtomicBool::new(false),
            cancel: AtomicBool::new(false),
            shortcut_active: AtomicBool::new(false),
        }
    }

    fn temp_file(tag: &str, contents: &[u8]) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "ocr4code-{tag}-{}-{unique}.png",
            std::process::id()
        ));
        std::fs::write(&path, contents).unwrap();
        path
    }

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

    #[test]
    fn an_unsupported_file_is_rejected_before_it_is_opened() {
        let err = validate_image_file(Path::new("/definitely/missing/notes.txt")).unwrap_err();
        assert_eq!(err.message, "不支持这个格式");
        assert!(err.hint.contains("PNG"));
    }

    #[test]
    fn files_over_the_size_limit_are_rejected() {
        assert!(check_image_size(MAX_IMAGE_BYTES).is_ok());
        let err = check_image_size(MAX_IMAGE_BYTES + 1).unwrap_err();
        assert_eq!(err.message, "图片太大了");
        assert!(err.hint.contains("40 MB"));
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

    #[test]
    fn fast_preview_switches_to_vision_without_changing_output_knobs() {
        let mut config = Settings::default().config();
        config.tile = true;
        config.max_tokens = 2048;
        config.format = Format::Markdown;

        let fast = fast_preview_config(&config);

        assert_eq!(fast.engine, EngineMode::Vision);
        assert!(!fast.tile);
        assert_eq!(fast.max_tokens, 2048);
        assert_eq!(fast.format, Format::Markdown);
    }

    #[test]
    fn a_second_job_is_rejected_while_one_is_running() {
        let state = test_state();
        assert!(claim_busy(&state).is_ok());
        assert!(!state.cancel.load(Ordering::SeqCst));
        let err = claim_busy(&state).unwrap_err();
        assert_eq!(err.message, "正在识别上一张图");
        state.busy.store(false, Ordering::SeqCst);
        assert!(claim_busy(&state).is_ok());
    }
}
