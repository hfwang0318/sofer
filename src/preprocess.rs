//! Normalize an image before it is handed to an engine.
//!
//! Everything here changes what the *model* sees, never how geometry is
//! interpreted: `geom`/`assemble` keep working in the normalized original's
//! pixel space, and [`Transform`] maps that space onto the (possibly cropped and
//! upscaled) model copy. The engine still only supplies text.
//!
//! The steps, in order:
//!
//! 1. decode with the EXIF orientation applied (camera photos and some exported
//!    PNGs carry it, and `image::load_from_memory` ignores it);
//! 2. flatten transparency onto white (a copied PNG can have a transparent
//!    background, which would otherwise be read as black ink);
//! 3. cap the decoded pixel count (a small compressed file can decode into
//!    hundreds of megapixels);
//! 4. crawl the content box and crop the background margins (the vision encoder
//!    resizes to a fixed budget, so a margin costs real character resolution);
//! 5. upscale text that is too small for the encoder (the `@1x` corpus is
//!    measurably worse than `@2x`: content 0.934 vs 0.955).

use anyhow::{Context, Result};
use image::imageops::FilterType;
use image::metadata::Orientation;
use image::{DynamicImage, GenericImageView, ImageDecoder, ImageFormat, ImageReader};
use std::io::Cursor;

/// Decoded-pixel ceiling used when the caller does not set one.
///
/// A 40 MiB PNG can decode into >100 megapixels; `geom` then allocates several
/// bytes per pixel and the process can be killed by the OS. 50 M px is above a
/// 5K screenshot (14.7 M) with a wide margin, and only large photos/scans hit it.
pub const DEFAULT_MAX_PIXELS: u64 = 50_000_000;

/// Rows whose ink height is below this are upscaled 2× before recognition.
///
/// Measured on the bench corpus: `@2x` screenshots have a line height around
/// 28–34 px, `@1x` around 14–17 px. The encoder's effective resolution is
/// better when a 1× screenshot is resampled than when the encoder downsamples it.
pub const SMALL_TEXT_LINE_HEIGHT: f32 = 16.0;

/// Multiply a small-text image by this factor. Only 2× is used: a non-integer
/// factor needs a second interpolation pass and gains nothing measurable.
const UPSCALE_FACTOR: f32 = 2.0;

/// A crop that loses less than this fraction of the area is skipped.
///
/// Measured on the 26-scene bench: cropping a 2–20% margin changed nothing for
/// the better and cost ~1 pp `text_acc` (`tree`, `light-ambiguous`, `pytest`),
/// because the model reacts to the frame size itself. Only a large empty
/// margin is worth the re-encode; below this threshold the original bytes are
/// forwarded unchanged and the request stays byte-identical to the old flow.
const MIN_TRIM_RATIO: f32 = 0.35;

/// Decoded image plus the facts that decide whether the original bytes can be
/// forwarded to the engine unchanged.
pub struct Decoded {
    pub image: DynamicImage,
    pub format: ImageFormat,
    /// EXIF orientation was applied, so the original bytes no longer describe
    /// the pixels in `image`.
    pub reoriented: bool,
}

/// Decode without resizing, applying the EXIF orientation when the format
/// carries one.
pub fn decode(bytes: &[u8]) -> Result<Decoded> {
    let reader = ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .context("reading the image header")?;
    let format = reader.format().unwrap_or(ImageFormat::Png);
    let mut decoder = reader.into_decoder().context("opening the image decoder")?;
    let orientation = decoder.orientation().unwrap_or(Orientation::NoTransforms);
    let mut image = DynamicImage::from_decoder(decoder).context("decoding the image")?;
    let reoriented = orientation != Orientation::NoTransforms;
    if reoriented {
        image.apply_orientation(orientation);
    }
    Ok(Decoded {
        image,
        format,
        reoriented,
    })
}

/// Composite an alpha channel onto white. Returns whether any pixel was
/// actually transparent (a fully opaque RGBA image keeps its original bytes
/// eligible for forwarding).
pub fn flatten_alpha(image: DynamicImage) -> (DynamicImage, bool) {
    if !matches!(
        image,
        DynamicImage::ImageRgba8(_)
            | DynamicImage::ImageRgba16(_)
            | DynamicImage::ImageLumaA8(_)
            | DynamicImage::ImageLumaA16(_)
    ) {
        return (image, false);
    }
    let rgba = image.to_rgba8();
    let mut changed = false;
    let mut out = image::RgbImage::new(rgba.width(), rgba.height());
    for (dst, src) in out.pixels_mut().zip(rgba.pixels()) {
        let alpha = src[3] as u32;
        if alpha != 255 {
            changed = true;
        }
        // White is the neutral background for both light and dark themes: a
        // transparent pixel has no ink, so it must not bias the background
        // median towards black.
        let mix = |c: u8| ((c as u32 * alpha + 255 * (255 - alpha)) / 255) as u8;
        *dst = image::Rgb([mix(src[0]), mix(src[1]), mix(src[2])]);
    }
    (DynamicImage::ImageRgb8(out), changed)
}

/// Downscale when the image exceeds `max_pixels`. `0` disables the cap.
pub fn cap_pixels(image: DynamicImage, max_pixels: u64) -> (DynamicImage, bool) {
    if max_pixels == 0 {
        return (image, false);
    }
    let (width, height) = image.dimensions();
    if u64::from(width) * u64::from(height) <= max_pixels {
        return (image, false);
    }
    let scale = (max_pixels as f64 / (f64::from(width) * f64::from(height))).sqrt();
    let new_width = ((f64::from(width) * scale).floor() as u32).max(1);
    let new_height = ((f64::from(height) * scale).floor() as u32).max(1);
    (
        image.resize_exact(new_width, new_height, FilterType::Lanczos3),
        true,
    )
}

/// The ink bounding box of a whole page, in the image's own coordinates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Bounds {
    pub x0: u32,
    pub y0: u32,
    pub x1: u32,
    pub y1: u32,
}

impl Bounds {
    pub fn width(self) -> u32 {
        self.x1.saturating_sub(self.x0)
    }
    pub fn height(self) -> u32 {
        self.y1.saturating_sub(self.y0)
    }
}

/// Content box from the detected row bands, with a small safety margin.
///
/// `None` when there is no ink at all (a blank frame is handled earlier, but a
/// fully uniform non-blank input can still reach here).
pub fn content_bounds(geom: &crate::geom::Geometry, pad_x: f32, pad_y: f32) -> Option<Bounds> {
    let first = geom.bands.first()?;
    let last = geom.bands.last()?;
    let mut min_x = f32::MAX;
    let mut max_x = 0.0f32;
    for band in &geom.bands {
        min_x = min_x.min(band.x0);
        max_x = max_x.max(band.x1);
    }
    if !min_x.is_finite() || max_x <= min_x {
        return None;
    }
    // Never grow past the frame: a margin that reaches the edge is a no-op.
    let x0 = (min_x - pad_x).floor().clamp(0.0, geom.width as f32) as u32;
    let x1 = (max_x + pad_x).ceil().clamp(0.0, geom.width as f32) as u32;
    let y0 = (first.y0 - pad_y).floor().clamp(0.0, geom.height as f32) as u32;
    let y1 = (last.y1 + pad_y).ceil().clamp(0.0, geom.height as f32) as u32;
    (x1 > x0 && y1 > y0).then_some(Bounds { x0, y0, x1, y1 })
}

/// Should the content box be used? Small crops are not worth a re-encode.
pub fn trim_is_worthwhile(image: &DynamicImage, bounds: Bounds) -> bool {
    let (width, height) = image.dimensions();
    let full = f64::from(width) * f64::from(height);
    if full <= 0.0 {
        return false;
    }
    let kept = f64::from(bounds.width()) * f64::from(bounds.height());
    ((full - kept) / full) as f32 >= MIN_TRIM_RATIO
}

/// Map a pixel region from the normalized original onto the model copy.
///
/// `dx`/`dy` are the crop origin; `scale` is the uniform upscale factor. A
/// single factor is enough because trimming is orthogonal and upscaling is
/// uniform.
#[derive(Debug, Clone, Copy)]
pub struct Transform {
    pub dx: u32,
    pub dy: u32,
    pub scale: f32,
}

impl Transform {
    pub const IDENTITY: Self = Self {
        dx: 0,
        dy: 0,
        scale: 1.0,
    };

    pub fn map_x(self, x: f32) -> f32 {
        (x - self.dx as f32) * self.scale
    }

    pub fn map_y(self, y: f32) -> f32 {
        (y - self.dy as f32) * self.scale
    }

    pub fn is_identity(self) -> bool {
        self.dx == 0 && self.dy == 0 && (self.scale - 1.0).abs() < f32::EPSILON
    }
}

/// Upscale when the detected line height says the text is below the encoder's
/// comfortable size.
///
/// `None` means "leave the image alone" — returning the input by value would
/// force a `clone()` of a frame that can be tens of megabytes, and this is the
/// common case.
pub fn upscale_small(
    image: &DynamicImage,
    line_height: f32,
    enabled: bool,
) -> Option<(DynamicImage, f32)> {
    if !enabled || line_height <= 0.0 || line_height >= SMALL_TEXT_LINE_HEIGHT {
        return None;
    }
    let (width, height) = image.dimensions();
    let new_width = ((width as f32) * UPSCALE_FACTOR).round() as u32;
    let new_height = ((height as f32) * UPSCALE_FACTOR).round() as u32;
    if new_width < 2 || new_height < 2 {
        return None;
    }
    Some((
        image.resize_exact(new_width, new_height, FilterType::Lanczos3),
        UPSCALE_FACTOR,
    ))
}

/// MIME type usable in a `data:` URL when forwarding the original bytes.
///
/// The llama.cpp multimodal path decodes with stb_image, which understands PNG
/// and JPEG but not WebP or TIFF; those must be re-encoded instead of forwarded.
pub fn forwardable_mime(format: ImageFormat) -> Option<&'static str> {
    match format {
        ImageFormat::Png => Some("image/png"),
        ImageFormat::Jpeg => Some("image/jpeg"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{Rgb, RgbImage, Rgba, RgbaImage};

    fn rgb(width: u32, height: u32) -> DynamicImage {
        DynamicImage::ImageRgb8(RgbImage::from_pixel(width, height, Rgb([250, 250, 250])))
    }

    #[test]
    fn transparent_pixels_are_composited_onto_white() {
        let mut rgba = RgbaImage::from_pixel(2, 1, Rgba([0, 0, 0, 0]));
        rgba.put_pixel(1, 0, Rgba([10, 20, 30, 255]));
        let (flat, changed) = flatten_alpha(DynamicImage::ImageRgba8(rgba));
        assert!(changed);
        let rgb = flat.to_rgb8();
        assert_eq!(rgb.get_pixel(0, 0), &Rgb([255, 255, 255]));
        assert_eq!(rgb.get_pixel(1, 0), &Rgb([10, 20, 30]));
    }

    #[test]
    fn a_fully_opaque_rgba_image_is_not_marked_as_changed() {
        let rgba = RgbaImage::from_pixel(2, 1, Rgba([1, 2, 3, 255]));
        let (_, changed) = flatten_alpha(DynamicImage::ImageRgba8(rgba));
        assert!(!changed);
    }

    #[test]
    fn the_pixel_cap_preserves_the_aspect_ratio() {
        let (capped, changed) = cap_pixels(rgb(4000, 3000), 3_000_000);
        assert!(changed);
        let (w, h) = capped.dimensions();
        assert!(u64::from(w) * u64::from(h) <= 3_000_000);
        assert!((w as f32 / h as f32 - 4.0 / 3.0).abs() < 0.01);
    }

    #[test]
    fn a_small_image_is_never_capped() {
        let (_, changed) = cap_pixels(rgb(100, 100), 50_000_000);
        assert!(!changed);
        let (_, unlimited) = cap_pixels(rgb(100, 100), 0);
        assert!(!unlimited);
    }

    #[test]
    fn the_transform_maps_crop_and_scale_in_one_step() {
        let t = Transform {
            dx: 10,
            dy: 20,
            scale: 2.0,
        };
        assert_eq!(t.map_x(30.0), 40.0);
        assert_eq!(t.map_y(40.0), 40.0);
        assert!(Transform::IDENTITY.is_identity());
        assert!(!t.is_identity());
    }

    #[test]
    fn only_png_and_jpeg_may_be_forwarded_byte_for_byte() {
        assert_eq!(forwardable_mime(ImageFormat::Png), Some("image/png"));
        assert_eq!(forwardable_mime(ImageFormat::Jpeg), Some("image/jpeg"));
        assert_eq!(forwardable_mime(ImageFormat::WebP), None);
        assert_eq!(forwardable_mime(ImageFormat::Tiff), None);
        assert_eq!(forwardable_mime(ImageFormat::Bmp), None);
    }

    #[test]
    fn upscaling_only_triggers_below_the_threshold() {
        assert!(upscale_small(&rgb(100, 100), 30.0, true).is_none());

        let (small, factor) = upscale_small(&rgb(100, 100), 12.0, true).expect("upscaled");
        assert_eq!(factor, 2.0);
        assert_eq!(small.dimensions(), (200, 200));

        assert!(upscale_small(&rgb(100, 100), 12.0, false).is_none());
    }
}
