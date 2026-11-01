//! App-drawn "captured" toast.
//!
//! A background capture has no visible feedback once the main window is hidden,
//! so a small frameless window fades in at the bottom centre of the monitor
//! under the cursor and hides itself after a couple of seconds. It is a real
//! Tauri window rather than an OS notification: notifications depend on system
//! permissions, can be silenced, and would carry a code preview we do not need.
//!
//! Rules this file owns: the window never takes keyboard focus, never accepts
//! mouse input, never stacks, and only appears when the main window is not in
//! front and the result already reached the clipboard.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::Duration;

use serde::Serialize;
use tauri::{
    AppHandle, Emitter, Manager, Monitor, PhysicalPosition, PhysicalSize, WebviewUrl,
    WebviewWindow, WebviewWindowBuilder,
};

const LABEL: &str = "toast";
const EVENT: &str = "toast:show";

/// Logical window size; the pill inside is narrower, the rest is transparent.
const WIDTH: f64 = 320.0;
const HEIGHT: f64 = 64.0;
/// Gap between the pill and the bottom of the monitor's work area.
const BOTTOM_MARGIN: f64 = 80.0;
/// Fallback hide in case the page never calls `toast_dismiss`.
const FALLBACK_HIDE: Duration = Duration::from_secs(5);

/// The most recent toast, so a freshly created webview can fetch it when its
/// listener was not registered yet.
static PENDING: Mutex<Option<ToastPayload>> = Mutex::new(None);
/// Bumped on every show; stale fade-out timers must not hide a newer toast.
static GENERATION: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ToastPayload {
    pub id: u64,
    pub lines: usize,
}

/// A rectangle in physical pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

/// Where the toast window goes, in physical pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Placement {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

/// Bottom centre of a monitor, `BOTTOM_MARGIN` logical pixels above its work
/// area. Pure so multi-monitor and HiDPI maths stay testable.
pub fn toast_placement(monitor: Rect, scale: f64) -> Placement {
    let scale = if scale.is_finite() && scale > 0.0 {
        scale
    } else {
        1.0
    };
    let width = (WIDTH * scale).round().max(1.0) as u32;
    let height = (HEIGHT * scale).round().max(1.0) as u32;
    let margin = (BOTTOM_MARGIN * scale).round().max(0.0) as i32;

    let free_x = (monitor.width as i32 - width as i32).max(0);
    let free_y = monitor.height as i32 - height as i32 - margin;
    Placement {
        x: monitor.x + free_x / 2,
        y: monitor.y + free_y.max(0),
        width,
        height,
    }
}

/// Shows the toast when the main window is not in front. Callers only need to
/// know the result reached the clipboard; everything else is decided here.
pub fn show_for_background(app: &AppHandle, lines: usize, enabled: bool) {
    if !enabled || main_in_foreground(app) {
        return;
    }
    show(app, lines);
}

fn main_in_foreground(app: &AppHandle) -> bool {
    app.get_webview_window("main")
        .map(|window| window.is_visible().unwrap_or(false) && window.is_focused().unwrap_or(false))
        .unwrap_or(false)
}

fn show(app: &AppHandle, lines: usize) {
    let id = GENERATION.fetch_add(1, Ordering::SeqCst) + 1;
    let payload = ToastPayload { id, lines };
    if let Ok(mut pending) = PENDING.lock() {
        *pending = Some(payload.clone());
    }

    // Windows and webviews must be created on the event-loop thread.
    let handle = app.clone();
    if let Err(err) = app.run_on_main_thread(move || {
        if let Err(err) = present(&handle, &payload) {
            eprintln!("toast: {err}");
        }
    }) {
        eprintln!("toast: cannot schedule the window: {err}");
        return;
    }

    let fallback = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(FALLBACK_HIDE);
        if GENERATION.load(Ordering::SeqCst) == id {
            hide(&fallback);
        }
    });
}

fn present(app: &AppHandle, payload: &ToastPayload) -> Result<(), String> {
    let monitor = cursor_monitor(app)?;
    let placement = toast_placement(
        Rect {
            x: monitor.work_area().position.x,
            y: monitor.work_area().position.y,
            width: monitor.work_area().size.width,
            height: monitor.work_area().size.height,
        },
        monitor.scale_factor(),
    );

    let window = match app.get_webview_window(LABEL) {
        Some(window) => window,
        None => create(app)?,
    };

    // Re-applied on every show: cheap, and keeps the contract if a platform
    // resets the flags with the window.
    window
        .set_ignore_cursor_events(true)
        .map_err(|e| format!("设置鼠标穿透失败：{e}"))?;
    window
        .set_focusable(false)
        .map_err(|e| format!("设置焦点策略失败：{e}"))?;
    window
        .set_position(PhysicalPosition::new(placement.x, placement.y))
        .map_err(|e| format!("定位识别完成弹窗失败：{e}"))?;
    window
        .set_size(PhysicalSize::new(placement.width, placement.height))
        .map_err(|e| format!("设置识别完成弹窗尺寸失败：{e}"))?;
    window
        .show()
        .map_err(|e| format!("显示识别完成弹窗失败：{e}"))?;

    // The first event can race the page load; the page fetches `toast_info`
    // on mount to cover that case.
    let _ = window.emit(EVENT, payload);
    Ok(())
}

fn cursor_monitor(app: &AppHandle) -> Result<Monitor, String> {
    if let Ok(point) = app.cursor_position() {
        if let Ok(Some(monitor)) = app.monitor_from_point(point.x, point.y) {
            return Ok(monitor);
        }
    }
    app.primary_monitor()
        .map_err(|e| format!("读取显示器信息失败：{e}"))?
        .ok_or_else(|| "没有可用的显示器".to_string())
}

fn create(app: &AppHandle) -> Result<WebviewWindow, String> {
    WebviewWindowBuilder::new(app, LABEL, WebviewUrl::App("toast.html".into()))
        .title("ocr4code — 识别完成")
        .decorations(false)
        // Clears the window *and* webview background layers. `transparent(true)`
        // alone leaves WKWebView's under-page background painted on macOS, which
        // shows up as a translucent rectangle behind the pill.
        .background_color(tauri::webview::Color(0, 0, 0, 0))
        .transparent(true)
        .always_on_top(true)
        .skip_taskbar(true)
        .resizable(false)
        .shadow(false)
        .focused(false)
        .visible(false)
        .inner_size(WIDTH, HEIGHT)
        .build()
        .map_err(|e| format!("创建识别完成弹窗失败：{e}"))
}

fn hide(app: &AppHandle) {
    if let Some(window) = app.get_webview_window(LABEL) {
        let _ = window.hide();
    }
}

/// Content for a page that mounted after the event was emitted.
#[tauri::command]
pub fn toast_info(app: AppHandle) -> Option<ToastPayload> {
    let visible = app
        .get_webview_window(LABEL)
        .and_then(|window| window.is_visible().ok())
        .unwrap_or(false);
    if !visible {
        return None;
    }
    PENDING.lock().ok().and_then(|pending| pending.clone())
}

/// Hides the window after the page's fade-out. `id` guards against a newer
/// capture arriving while the previous toast was still fading.
#[tauri::command]
pub fn toast_dismiss(app: AppHandle, id: u64) {
    if GENERATION.load(Ordering::SeqCst) == id {
        hide(&app);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sits_at_the_bottom_centre_of_a_1080p_screen() {
        let placement = toast_placement(
            Rect {
                x: 0,
                y: 0,
                width: 1920,
                height: 1080,
            },
            1.0,
        );
        assert_eq!(
            placement,
            Placement {
                x: 800,
                y: 936,
                width: 320,
                height: 64,
            }
        );
    }

    #[test]
    fn scales_with_a_hidpi_monitor() {
        let placement = toast_placement(
            Rect {
                x: 0,
                y: 0,
                width: 3840,
                height: 2160,
            },
            2.0,
        );
        assert_eq!(
            placement,
            Placement {
                x: 1600,
                y: 1872,
                width: 640,
                height: 128,
            }
        );
    }

    #[test]
    fn follows_a_monitor_with_a_negative_origin() {
        let left = toast_placement(
            Rect {
                x: -1920,
                y: 0,
                width: 1920,
                height: 1080,
            },
            1.0,
        );
        assert_eq!(left.x, -1120);
        assert_eq!(left.y, 936);

        let above = toast_placement(
            Rect {
                x: 0,
                y: -1280,
                width: 1920,
                height: 1280,
            },
            1.0,
        );
        assert_eq!(above.y, -144);
    }

    #[test]
    fn keeps_a_tiny_monitor_inside_its_bounds() {
        let placement = toast_placement(
            Rect {
                x: 0,
                y: 0,
                width: 240,
                height: 80,
            },
            1.0,
        );
        assert_eq!(placement.x, 0);
        assert_eq!(placement.y, 0);
    }

    #[test]
    fn a_broken_scale_factor_falls_back_to_one() {
        let expected = toast_placement(
            Rect {
                x: 0,
                y: 0,
                width: 1920,
                height: 1080,
            },
            1.0,
        );
        assert_eq!(
            toast_placement(
                Rect {
                    x: 0,
                    y: 0,
                    width: 1920,
                    height: 1080,
                },
                0.0,
            ),
            expected
        );
        assert_eq!(
            toast_placement(
                Rect {
                    x: 0,
                    y: 0,
                    width: 1920,
                    height: 1080,
                },
                f64::NAN,
            ),
            expected
        );
    }
}
