//! Clipboard images and file validation: extension and size checks, plus the
//! drag payload the page renders while a file hovers over the window.

use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;

use tauri::{AppHandle, Emitter, Manager};

use crate::settings::AppState;

use super::{is_supported_image_path, now_ms, DragPayload, InputError, MAX_IMAGE_BYTES};

/// Copy the clipboard image to a temp PNG. A failure here must release the
/// busy flag and leave no partial file behind.
pub(super) fn prepare_clipboard_image(app: &AppHandle) -> Result<PathBuf, InputError> {
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

pub(super) fn claim_busy(state: &AppState) -> Result<(), InputError> {
    if state.busy.swap(true, Ordering::SeqCst) {
        return Err(InputError::new("正在识别上一张图", "等这次识别结束再试。"));
    }
    state.cancel.store(false, Ordering::SeqCst);
    Ok(())
}

pub(super) fn validate_image_file(path: &Path) -> Result<(), InputError> {
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
    Ok(dir.join(format!("clipboard-{}-{}.png", std::process::id(), now_ms())))
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
            return Err(InputError::new("剪贴板读不出图片", "复制一张图片后重试。"));
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

#[cfg(test)]
mod tests {
    use super::*;

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
    fn a_second_job_is_rejected_while_one_is_running() {
        let state = test_state();
        assert!(claim_busy(&state).is_ok());
        assert!(!state.cancel.load(Ordering::SeqCst));
        let err = claim_busy(&state).unwrap_err();
        assert_eq!(err.message, "正在识别上一张图");
        state.busy.store(false, Ordering::SeqCst);
        assert!(claim_busy(&state).is_ok());
    }

    fn test_state() -> AppState {
        use std::sync::atomic::AtomicBool;

        AppState {
            settings: std::sync::Mutex::new(crate::settings::Settings::default()),
            busy: AtomicBool::new(false),
            cancel: AtomicBool::new(false),
            shortcut_active: AtomicBool::new(false),
        }
    }
}
