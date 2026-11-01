//! The recognition flow: resolve the engine, stream partial text, run the fast
//! Apple Vision preview, build the result, and classify errors for the UI.

use std::io::Cursor;
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use base64::{engine::general_purpose::STANDARD, Engine as _};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_clipboard_manager::ClipboardExt;

use ocr4code::engine::tesseract;
use ocr4code::pipeline::{Config, EngineMode};
use ocr4code::render::{self, Format};

use crate::engine;
use crate::remote;
use crate::settings::Settings;

use super::{current_settings, now_ms, OcrPartial, OcrResult};

/// One token per IPC event would flood the webview; ~8 updates a second is
/// enough for a preview. The first delta is always sent immediately.
const PARTIAL_EMIT_INTERVAL: Duration = Duration::from_millis(120);

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct OcrFailure {
    message: String,
    hint: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    code: Option<String>,
}

/// Same classification for every entry point: the capture page renders one
/// error panel and the shortcut path never shows a different vocabulary.
pub(super) fn report_error(app: &AppHandle, err: &anyhow::Error) {
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

pub(super) fn retry_max_tokens(current: u32) -> u32 {
    (current / 2).max(1024)
}

/// Resolve the engine and pin the config to what actually answered.
///
/// `ensure_ready` already proved that the engine is up, so leaving the mode on
/// `auto` would health-check it a second time (800 ms) and a slow-but-healthy
/// server could silently fall back to tesseract. `auto` only falls back when
/// `ensure_ready` itself failed. Returns `Ok(false)` when the user cancelled.
pub(super) fn prepare_engine(
    app: &AppHandle,
    settings: &Settings,
    config: &mut Config,
) -> Result<bool> {
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
pub(super) fn emit_fast_preview(
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

pub(super) fn recognize_to_result(
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

// ---------------------------------------------------------------------------
// Selection warm-up
// ---------------------------------------------------------------------------

/// Ask a ready engine for one token while the user is still dragging, so an
/// idle-sleeping server is awake by the time the screenshot arrives. Best
/// effort: it never blocks selection and never touches `busy` / `cancel`.
pub(super) fn spawn_warm_up(app: &AppHandle) {
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
        return "在系统设置 → 隐私与安全性 → 屏幕录制中勾选 ocr4code，然后重启应用。".into();
    }
    if message.contains("远程") || message.contains("服务器") {
        return "到设置 → OCR → 部署到服务器里检查连接，或重新部署。".into();
    }
    if message.contains("E_BLANK") {
        #[cfg(target_os = "macos")]
        return "如果框选区域本来就有文字，请检查屏幕录制权限，然后重启应用。".into();
        #[cfg(not(target_os = "macos"))]
        return "请重新框选包含文字的区域。".into();
    }
    let lower = message.to_lowercase();
    if lower.contains("no engine available") || lower.contains("starting the ocr engine") {
        "先下载 OCR 模型，或安装 tesseract 后重试。".into()
    } else if lower.contains("not reachable") || lower.contains("llama") {
        "确认服务正在运行，并检查设置里的地址。".into()
    } else if lower.contains("permission") || lower.contains("screen") {
        "在系统设置 → 隐私与安全性 → 屏幕录制中勾选 ocr4code，然后重启应用。".into()
    } else {
        "检查设置后重试。".into()
    }
}

pub(super) struct CaptureInspection {
    pub(super) blank: bool,
    pub(super) thumbnail: Option<String>,
}

/// A capture made without screen-recording permission is an almost uniformly
/// colored frame. Decode once to check that and to build the result thumbnail.
pub(super) fn inspect_capture(path: &Path) -> CaptureInspection {
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
}
