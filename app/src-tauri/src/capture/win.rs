//! Windows region selection without the Snipping Tool.
//!
//! We grab the monitor under the cursor with GDI *before* showing anything, put
//! that frame in our own always-on-top Tauri window, and crop the original frame
//! when the user releases the mouse. The system screenshot UI is never invoked.

use std::io::Cursor;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use anyhow::{anyhow, Context, Result};
use base64::{engine::general_purpose::STANDARD, Engine as _};
use image::{imageops, RgbaImage};
use tauri::{
    AppHandle, Manager, PhysicalPosition, PhysicalSize, WebviewUrl, WebviewWindow,
    WebviewWindowBuilder,
};
use windows::Win32::Foundation::POINT;
use windows::Win32::Graphics::Gdi::{
    BitBlt, CreateCompatibleBitmap, CreateCompatibleDC, DeleteDC, DeleteObject, GetDC, GetDIBits,
    GetMonitorInfoW, MonitorFromPoint, ReleaseDC, SelectObject, BITMAPINFO, BITMAPINFOHEADER,
    BI_RGB, CAPTUREBLT, DIB_RGB_COLORS, MONITORINFO, MONITOR_DEFAULTTONEAREST, SRCCOPY,
};
use windows::Win32::UI::WindowsAndMessaging::GetCursorPos;

use super::{CaptureOutcome, OverlayInfo};

const OVERLAY_LABEL: &str = "region-overlay";
const MAX_OVERLAY_SECONDS: u64 = 120;

#[derive(Clone, Copy)]
struct MonitorRect {
    left: i32,
    top: i32,
    width: i32,
    height: i32,
}

struct Frame {
    rgba: Vec<u8>,
    width: u32,
    height: u32,
}

struct Selection {
    x: i32,
    y: i32,
    width: i32,
    height: i32,
}

struct Pending {
    frame: Frame,
    preview: String,
    sender: Sender<Option<Selection>>,
}

static PENDING: Mutex<Option<Pending>> = Mutex::new(None);

pub(super) fn overlay_info() -> Result<OverlayInfo, String> {
    let pending = PENDING.lock().map_err(|_| "截图状态锁已损坏".to_string())?;
    let pending = pending
        .as_ref()
        .ok_or_else(|| "截图会话不存在".to_string())?;
    Ok(OverlayInfo {
        image: pending.preview.clone(),
    })
}

pub(super) fn overlay_finish(
    window: WebviewWindow,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
    scale: f64,
) -> Result<(), String> {
    let scale = if scale.is_finite() && scale > 0.0 {
        scale
    } else {
        window
            .scale_factor()
            .map_err(|e| format!("读取截图缩放比例失败：{e}"))?
    };
    let selection = Selection {
        x: (x * scale).round() as i32,
        y: (y * scale).round() as i32,
        width: (width * scale).round() as i32,
        height: (height * scale).round() as i32,
    };
    if selection.width < 2 || selection.height < 2 {
        return overlay_cancel();
    }
    let sender = {
        let pending = PENDING.lock().map_err(|_| "截图状态锁已损坏".to_string())?;
        pending
            .as_ref()
            .ok_or_else(|| "截图会话不存在".to_string())?
            .sender
            .clone()
    };
    sender
        .send(Some(selection))
        .map_err(|_| "截图会话已结束".to_string())
}

pub(super) fn overlay_cancel() -> Result<(), String> {
    let sender = {
        let pending = PENDING.lock().map_err(|_| "截图状态锁已损坏".to_string())?;
        pending.as_ref().map(|pending| pending.sender.clone())
    };
    if let Some(sender) = sender {
        let _ = sender.send(None);
    }
    Ok(())
}

pub(super) fn interactive_capture(
    app: &AppHandle,
    dest: &Path,
    cancel: &AtomicBool,
) -> Result<CaptureOutcome> {
    if cancel.load(Ordering::SeqCst) {
        return Ok(CaptureOutcome::Cancelled);
    }

    let monitor = monitor_under_cursor()?;
    let frame = capture_rect(monitor)?;
    let preview = preview_data_url(&frame)?;
    let (sender, receiver) = mpsc::channel();
    {
        let mut pending = PENDING.lock().map_err(|_| anyhow!("截图状态锁已损坏"))?;
        *pending = Some(Pending {
            frame,
            preview,
            sender,
        });
    }

    let (ready_sender, ready_receiver) = mpsc::channel();
    let app_for_overlay = app.clone();
    if let Err(err) = app.run_on_main_thread(move || {
        let _ = ready_sender.send(create_overlay(&app_for_overlay, monitor));
    }) {
        PENDING.lock().ok().and_then(|mut pending| pending.take());
        return Err(anyhow!("无法调度截图浮层：{err}"));
    }

    match ready_receiver.recv_timeout(Duration::from_secs(10)) {
        Ok(Ok(())) => {}
        Ok(Err(err)) => {
            PENDING.lock().ok().and_then(|mut pending| pending.take());
            return Err(anyhow!(err));
        }
        Err(_) => {
            PENDING.lock().ok().and_then(|mut pending| pending.take());
            return Err(anyhow!("创建截图浮层超时"));
        }
    }

    let started = Instant::now();
    loop {
        if cancel.load(Ordering::SeqCst)
            || started.elapsed() > Duration::from_secs(MAX_OVERLAY_SECONDS)
        {
            close_overlay(app);
            PENDING.lock().ok().and_then(|mut pending| pending.take());
            return Ok(CaptureOutcome::Cancelled);
        }

        match receiver.recv_timeout(Duration::from_millis(100)) {
            Ok(Some(selection)) => {
                close_overlay(app);
                let pending = PENDING
                    .lock()
                    .map_err(|_| anyhow!("截图状态锁已损坏"))?
                    .take()
                    .ok_or_else(|| anyhow!("截图会话已结束"))?;
                let image = crop(&pending.frame, selection)?;
                image.save(dest).context("保存截图失败")?;
                return Ok(CaptureOutcome::Selected);
            }
            Ok(None) => {
                close_overlay(app);
                PENDING.lock().ok().and_then(|mut pending| pending.take());
                return Ok(CaptureOutcome::Cancelled);
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => {
                close_overlay(app);
                PENDING.lock().ok().and_then(|mut pending| pending.take());
                return Ok(CaptureOutcome::Cancelled);
            }
        }
    }
}

fn create_overlay(app: &AppHandle, monitor: MonitorRect) -> Result<(), String> {
    if let Some(existing) = app.get_webview_window(OVERLAY_LABEL) {
        let _ = existing.close();
    }
    let window =
        WebviewWindowBuilder::new(app, OVERLAY_LABEL, WebviewUrl::App("overlay.html".into()))
            .title("ocr4code — 选择截图区域")
            .decorations(false)
            .resizable(false)
            .always_on_top(true)
            .skip_taskbar(true)
            .shadow(false)
            .visible(false)
            .build()
            .map_err(|e| format!("创建截图浮层失败：{e}"))?;

    window
        .set_position(PhysicalPosition::new(monitor.left, monitor.top))
        .map_err(|e| format!("定位截图浮层失败：{e}"))?;
    window
        .set_size(PhysicalSize::new(
            monitor.width.max(1) as u32,
            monitor.height.max(1) as u32,
        ))
        .map_err(|e| format!("设置截图浮层尺寸失败：{e}"))?;
    window
        .show()
        .map_err(|e| format!("显示截图浮层失败：{e}"))?;
    window
        .set_focus()
        .map_err(|e| format!("聚焦截图浮层失败：{e}"))?;
    Ok(())
}

fn close_overlay(app: &AppHandle) {
    if let Some(window) = app.get_webview_window(OVERLAY_LABEL) {
        let _ = window.close();
    }
}

fn monitor_under_cursor() -> Result<MonitorRect> {
    unsafe {
        let mut point = POINT::default();
        GetCursorPos(&mut point).map_err(|e| anyhow!("读取鼠标位置失败：{e}"))?;
        let monitor = MonitorFromPoint(point, MONITOR_DEFAULTTONEAREST);
        let mut info = MONITORINFO {
            cbSize: std::mem::size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        if !GetMonitorInfoW(monitor, &mut info).as_bool() {
            return Err(anyhow!("读取显示器信息失败"));
        }
        let rect = info.rcMonitor;
        let width = rect.right - rect.left;
        let height = rect.bottom - rect.top;
        if width <= 0 || height <= 0 {
            return Err(anyhow!("显示器尺寸无效"));
        }
        Ok(MonitorRect {
            left: rect.left,
            top: rect.top,
            width,
            height,
        })
    }
}

fn capture_rect(monitor: MonitorRect) -> Result<Frame> {
    unsafe {
        let screen = GetDC(None);
        if screen.0.is_null() {
            return Err(anyhow!("无法获取屏幕画布"));
        }
        let memory = CreateCompatibleDC(Some(screen));
        if memory.0.is_null() {
            ReleaseDC(None, screen);
            return Err(anyhow!("无法创建截图画布"));
        }
        let bitmap = CreateCompatibleBitmap(screen, monitor.width, monitor.height);
        if bitmap.0.is_null() {
            let _ = DeleteDC(memory);
            ReleaseDC(None, screen);
            return Err(anyhow!("无法创建截图位图"));
        }
        let old = SelectObject(memory, windows::Win32::Graphics::Gdi::HGDIOBJ(bitmap.0));
        if let Err(err) = BitBlt(
            memory,
            0,
            0,
            monitor.width,
            monitor.height,
            Some(screen),
            monitor.left,
            monitor.top,
            SRCCOPY | CAPTUREBLT,
        ) {
            let _ = SelectObject(memory, old);
            let _ = DeleteObject(windows::Win32::Graphics::Gdi::HGDIOBJ(bitmap.0));
            let _ = DeleteDC(memory);
            ReleaseDC(None, screen);
            return Err(anyhow!("抓取屏幕失败：{err}"));
        }

        let mut info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: monitor.width,
                biHeight: -monitor.height,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut bgra = vec![0u8; (monitor.width as usize) * (monitor.height as usize) * 4];
        let _ = SelectObject(memory, old);
        let lines = GetDIBits(
            memory,
            bitmap,
            0,
            monitor.height as u32,
            Some(bgra.as_mut_ptr().cast()),
            &mut info,
            DIB_RGB_COLORS,
        );

        let _ = DeleteObject(windows::Win32::Graphics::Gdi::HGDIOBJ(bitmap.0));
        let _ = DeleteDC(memory);
        ReleaseDC(None, screen);

        if lines == 0 {
            return Err(anyhow!("读取屏幕像素失败"));
        }
        for pixel in bgra.chunks_exact_mut(4) {
            pixel.swap(0, 2);
            pixel[3] = 255;
        }
        Ok(Frame {
            rgba: bgra,
            width: monitor.width as u32,
            height: monitor.height as u32,
        })
    }
}

fn preview_data_url(frame: &Frame) -> Result<String> {
    let mut image = RgbaImage::from_raw(frame.width, frame.height, frame.rgba.clone())
        .ok_or_else(|| anyhow!("截图尺寸不一致"))?;
    // Keep the preview at native resolution for 4K/5K displays; only very wide
    // virtual screens are reduced. The crop always uses the untouched frame.
    const MAX_PREVIEW_WIDTH: u32 = 5120;
    if image.width() > MAX_PREVIEW_WIDTH {
        let height = ((image.height() as u64 * MAX_PREVIEW_WIDTH as u64) / image.width() as u64)
            .max(1) as u32;
        image = imageops::resize(
            &image,
            MAX_PREVIEW_WIDTH,
            height,
            imageops::FilterType::Lanczos3,
        );
    }
    let mut bytes = Cursor::new(Vec::new());
    image.write_to(&mut bytes, image::ImageFormat::Png)?;
    Ok(format!(
        "data:image/png;base64,{}",
        STANDARD.encode(bytes.into_inner())
    ))
}

fn crop(frame: &Frame, selection: Selection) -> Result<RgbaImage> {
    let x = selection.x.clamp(0, frame.width as i32 - 1);
    let y = selection.y.clamp(0, frame.height as i32 - 1);
    let width = selection.width.min(frame.width as i32 - x).max(1);
    let height = selection.height.min(frame.height as i32 - y).max(1);
    let mut rgba = Vec::with_capacity((width as usize) * (height as usize) * 4);
    for row in 0..height {
        let start = (((y + row) as usize) * (frame.width as usize) + x as usize) * 4;
        rgba.extend_from_slice(&frame.rgba[start..start + (width as usize) * 4]);
    }
    RgbaImage::from_raw(width as u32, height as u32, rgba).ok_or_else(|| anyhow!("裁剪截图失败"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crop_keeps_the_selected_pixels() {
        let frame = Frame {
            rgba: (0..(4 * 4 * 4)).map(|value| value as u8).collect(),
            width: 4,
            height: 4,
        };
        let image = crop(
            &frame,
            Selection {
                x: 1,
                y: 1,
                width: 2,
                height: 2,
            },
        )
        .unwrap();
        assert_eq!(image.dimensions(), (2, 2));
        assert_eq!(image.get_pixel(0, 0).0, [20, 21, 22, 23]);
    }
}
