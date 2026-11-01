//! Native window chrome that CSS cannot express.
//!
//! The main window is transparent and undecorated: its rounded shape comes from
//! the CSS surface, so every corner pixel outside the radius is transparent.
//! macOS still derives the native shadow from the window rectangle, so the
//! system shadow paints a straight dark line along the frame that stays visible
//! wherever the CSS surface curves away: four right-angle lines at the corners.
//!
//! Giving the window's content view the same corner radius clips the native
//! surface to the visible shape, and invalidating the shadow makes macOS
//! recompute it from that shape. The result keeps the native shadow and removes
//! the straight lines.
//!
//! Keep [`SURFACE_RADIUS`] in sync with `.app` in `src/styles/shell.css`.

use tauri::WebviewWindow;

/// `.app`'s `border-radius`, in CSS pixels.
#[cfg(target_os = "macos")]
pub const SURFACE_RADIUS: f64 = 16.0;

/// The page is zoomed by the UI scale, so the on-screen radius is the CSS
/// radius times the zoom. One extra pixel keeps the clip from biting into the
/// CSS edge's antialiasing.
#[cfg(target_os = "macos")]
pub fn corner_radius(ui_scale: f64) -> f64 {
    SURFACE_RADIUS * ui_scale.clamp(1.0, 1.3) + 1.0
}

/// Clips the native window surface to the rounded corner so the system shadow
/// follows it instead of the window rectangle.
#[cfg(target_os = "macos")]
pub fn round_native_corners(window: &WebviewWindow, ui_scale: f64) {
    use objc2::msg_send;
    use objc2::runtime::AnyObject;

    let (Ok(ns_window), Ok(view)) = (window.ns_window(), window.ns_view()) else {
        return;
    };
    let radius = corner_radius(ui_scale);
    // SAFETY: `ns_window`/`ns_view` are the live window and content view, and
    // this runs on the main thread (window construction and the settings
    // command both execute there). `setWantsLayer:`, `layer`, `CALayer`'s
    // corner settings and `invalidateShadow` are ordinary AppKit calls.
    unsafe {
        let ns_window = ns_window as *mut AnyObject;
        let view = view as *mut AnyObject;
        let _: () = msg_send![view, setWantsLayer: true];
        let layer: *mut AnyObject = msg_send![view, layer];
        if layer.is_null() {
            return;
        }
        let _: () = msg_send![layer, setCornerRadius: radius];
        let _: () = msg_send![layer, setMasksToBounds: true];
        // The shadow was computed for the old rectangular shape; without this
        // the straight corner lines stay on screen.
        let _: () = msg_send![ns_window, invalidateShadow];
    }
}

/// Windows keeps its own native rounded corner handling; nothing to do.
#[cfg(not(target_os = "macos"))]
pub fn round_native_corners(_window: &WebviewWindow, _ui_scale: f64) {}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::*;

    #[test]
    fn radius_follows_the_ui_scale() {
        assert_eq!(corner_radius(1.0), SURFACE_RADIUS + 1.0);
        assert_eq!(corner_radius(1.15), SURFACE_RADIUS * 1.15 + 1.0);
        assert_eq!(corner_radius(1.3), SURFACE_RADIUS * 1.3 + 1.0);
    }

    #[test]
    fn radius_clamps_out_of_range_scales() {
        assert_eq!(corner_radius(0.5), SURFACE_RADIUS + 1.0);
        assert_eq!(corner_radius(2.0), SURFACE_RADIUS * 1.3 + 1.0);
    }
}
