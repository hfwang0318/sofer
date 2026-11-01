//! Engine abstraction: anything that can turn an image into text lines.
//!
//! Engines differ wildly in what they give back (bbox or not, confidence or not),
//! so `RawLine` keeps those optional and `assemble` decides how to use them.

pub mod llamacpp;
pub mod mock;
pub mod tesseract;

use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use anyhow::{Context, Result};
use image::DynamicImage;
use serde::{Deserialize, Serialize};

use crate::geom::Rect;

#[derive(Debug, Clone)]
pub struct RawLine {
    pub text: String,
    pub bbox: Option<Rect>,
    pub conf: Option<f32>,
}

impl RawLine {
    pub fn plain(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            bbox: None,
            conf: None,
        }
    }
    pub fn with_box(text: impl Into<String>, bbox: Rect, conf: Option<f32>) -> Self {
        Self {
            text: text.into(),
            bbox: Some(bbox),
            conf,
        }
    }
}

#[derive(Debug, Clone)]
pub struct EngineOutput {
    pub engine: String,
    pub lines: Vec<RawLine>,
    pub ms: u64,
}

impl EngineOutput {
    pub fn has_boxes(&self) -> bool {
        !self.lines.is_empty() && self.lines.iter().all(|l| l.bbox.is_some())
    }
}

/// Quality used by `jpeg` when the caller does not specify one.
pub const DEFAULT_JPEG_QUALITY: u8 = 85;

/// How an image is turned into bytes for an engine that has no source file.
///
/// The default is [`ImageFormat::Png`], which uses the same encoder settings as
/// the old `crop.save(tmp.png)` tile path — so the bytes sent to the model are
/// unchanged unless the caller asks for JPEG.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ImageFormat {
    Png,
    /// JPEG at `quality` (1–100): smaller uploads for photo-like screenshots.
    Jpeg { quality: u8 },
}

impl Default for ImageFormat {
    fn default() -> Self {
        Self::Png
    }
}

impl ImageFormat {
    pub fn mime(self) -> &'static str {
        match self {
            Self::Png => "image/png",
            Self::Jpeg { .. } => "image/jpeg",
        }
    }

    pub fn extension(self) -> &'static str {
        match self {
            Self::Png => "png",
            Self::Jpeg { .. } => "jpg",
        }
    }

    /// Encode a decoded image in memory. No temporary file is involved.
    pub fn encode(self, img: &DynamicImage) -> Result<Vec<u8>> {
        let mut buf: Vec<u8> = Vec::new();
        match self {
            Self::Png => img
                .write_to(Cursor::new(&mut buf), image::ImageFormat::Png)
                .context("encoding image as png")?,
            Self::Jpeg { quality } => {
                let encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(
                    &mut buf,
                    quality.clamp(1, 100),
                );
                img.write_with_encoder(encoder)
                    .context("encoding image as jpeg")?;
            }
        }
        Ok(buf)
    }
}

impl std::str::FromStr for ImageFormat {
    type Err = String;

    /// Accepts `png`, `jpeg` / `jpg`, and `jpeg:<1-100>` / `jpg:<1-100>`.
    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        let s = s.trim().to_ascii_lowercase();
        let (kind, quality) = match s.split_once(':') {
            Some((kind, quality)) => (kind, Some(quality)),
            None => (s.as_str(), None),
        };
        match kind {
            "png" => match quality {
                None => Ok(Self::Png),
                Some(_) => Err(format!("png takes no quality (got {s:?})")),
            },
            "jpeg" | "jpg" => {
                let quality = match quality {
                    Some(q) => q
                        .parse::<u8>()
                        .map_err(|_| format!("invalid jpeg quality {q:?} (expected 1-100)"))?,
                    None => DEFAULT_JPEG_QUALITY,
                };
                if !(1..=100).contains(&quality) {
                    return Err(format!("jpeg quality must be 1-100, got {quality}"));
                }
                Ok(Self::Jpeg { quality })
            }
            _ => Err(format!(
                "unknown image format {s:?} (expected png, jpeg or jpeg:<1-100>)"
            )),
        }
    }
}

impl std::fmt::Display for ImageFormat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Png => f.write_str("png"),
            Self::Jpeg { quality } => write!(f, "jpeg:{quality}"),
        }
    }
}

/// One image on its way to an engine.
///
/// `source` is the still-encoded input (bytes + MIME) when the caller already
/// read one from disk — the whole-image path. Forwarding it verbatim keeps that
/// request byte-identical to the old path-based flow. Crops have no source and
/// are encoded from pixels, in memory.
#[derive(Clone, Copy)]
pub struct ImageInput<'a> {
    pub image: &'a DynamicImage,
    pub source: Option<(&'a [u8], &'a str)>,
}

impl<'a> ImageInput<'a> {
    pub fn from_pixels(image: &'a DynamicImage) -> Self {
        Self {
            image,
            source: None,
        }
    }

    pub fn with_source(image: &'a DynamicImage, bytes: &'a [u8], mime: &'a str) -> Self {
        Self {
            image,
            source: Some((bytes, mime)),
        }
    }
}

pub trait Engine: Send + Sync {
    fn name(&self) -> &str;
    fn recognize(&self, image: &Path) -> Result<EngineOutput>;

    /// Recognise an already-decoded image.
    ///
    /// The default is for engines that can only consume a *file* (a subprocess
    /// such as tesseract): reuse the caller's original bytes when there are any,
    /// otherwise encode with `format`; write them to a process-unique temp file
    /// and delegate to [`Engine::recognize`]. The file is removed afterwards.
    ///
    /// Engines that can work in memory override this so tiles never touch disk.
    fn recognize_image(&self, input: &ImageInput<'_>, format: ImageFormat) -> Result<EngineOutput> {
        let encoded;
        let (bytes, ext) = match input.source {
            Some((bytes, mime)) => (bytes, extension_for_mime(mime)),
            None => {
                encoded = format.encode(input.image)?;
                (encoded.as_slice(), format.extension())
            }
        };
        let tmp = TempImage::write(bytes, ext)?;
        self.recognize(tmp.path())
    }
}

/// MIME type used when forwarding a file the way the path-based flow did.
pub fn mime_for_path(path: &Path) -> &'static str {
    match path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .as_deref()
    {
        Some("jpg") | Some("jpeg") => "image/jpeg",
        Some("webp") => "image/webp",
        _ => "image/png",
    }
}

fn extension_for_mime(mime: &str) -> &'static str {
    match mime {
        "image/jpeg" => "jpg",
        "image/webp" => "webp",
        _ => "png",
    }
}

/// A temp file that deletes itself. The name carries the pid *and* a counter, so
/// concurrent images/tiles in one process cannot overwrite each other.
struct TempImage {
    path: PathBuf,
}

impl TempImage {
    fn write(bytes: &[u8], ext: &str) -> Result<Self> {
        static SEQ: AtomicU64 = AtomicU64::new(0);
        let seq = SEQ.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "ocr4code-input-{}-{seq}.{ext}",
            std::process::id()
        ));
        std::fs::write(&path, bytes)
            .with_context(|| format!("writing temporary image {}", path.display()))?;
        Ok(Self { path })
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempImage {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

/// Which engine the user asked for.
#[derive(Debug, Clone)]
pub enum Spec {
    /// deterministic engine that echoes known text (tests, debugging the layout layer)
    Mock { text: Option<String>, text_file: Option<std::path::PathBuf> },
    Tesseract { lang: String, psm: u32 },
    LlamaCpp { url: String, prompt: String, max_tokens: u32 },
    /// pick the best available at runtime
    Auto,
}

impl Spec {
    pub fn build(&self) -> Result<Box<dyn Engine>> {
        match self {
            Spec::Mock { text, text_file } => Ok(Box::new(mock::Mock::new(
                text.clone(),
                text_file.clone(),
            )?)),
            Spec::Tesseract { lang, psm } => Ok(Box::new(tesseract::Tesseract::new(
                lang.clone(),
                *psm,
            ))),
            Spec::LlamaCpp {
                url,
                prompt,
                max_tokens,
            } => Ok(Box::new(llamacpp::LlamaCpp::new(
                url.clone(),
                prompt.clone(),
                *max_tokens,
            ))),
            Spec::Auto => {
                let url = llamacpp::default_url();
                let llama = llamacpp::LlamaCpp::new(url, llamacpp::DEFAULT_PROMPT.into(), 3072);
                if llama.health() {
                    return Ok(Box::new(llama));
                }
                if tesseract::available() {
                    return Ok(Box::new(tesseract::Tesseract::new("eng".into(), 6)));
                }
                anyhow::bail!(
                    "no engine available: start `llama-server` (see `ocr4code doctor`) \
                     or install tesseract"
                )
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{DynamicImage, Rgb, RgbImage};
    use std::sync::Mutex;

    fn sample() -> DynamicImage {
        let mut img = RgbImage::from_pixel(24, 16, Rgb([250, 250, 250]));
        for x in 0..12 {
            img.put_pixel(x, 4, Rgb([10, 20, 30]));
        }
        DynamicImage::ImageRgb8(img)
    }

    /// An engine that can only recognise a file, like tesseract.
    struct FileEngine {
        seen: Mutex<Vec<std::path::PathBuf>>,
    }

    impl Engine for FileEngine {
        fn name(&self) -> &str {
            "file"
        }

        fn recognize(&self, image: &Path) -> Result<EngineOutput> {
            self.seen.lock().unwrap().push(image.to_path_buf());
            Ok(EngineOutput {
                engine: "file".into(),
                lines: vec![RawLine::plain("line")],
                ms: 0,
            })
        }
    }

    #[test]
    fn png_encoding_matches_the_old_save_path_byte_for_byte() {
        let img = sample();
        let encoded = ImageFormat::Png.encode(&img).expect("encode");

        let tmp =
            std::env::temp_dir().join(format!("ocr4code-test-png-{}.png", std::process::id()));
        img.save(&tmp).expect("save");
        let on_disk = std::fs::read(&tmp).expect("read");
        let _ = std::fs::remove_file(&tmp);

        assert_eq!(encoded, on_disk, "in-memory PNG must equal crop.save() bytes");
        // And the encoded bytes are a real image with identical pixels.
        let back = image::load_from_memory(&encoded).expect("decode");
        assert_eq!(back.to_rgb8(), img.to_rgb8());
    }

    #[test]
    fn jpeg_option_is_parseable_and_clamped_to_a_usable_range() {
        assert_eq!("png".parse::<ImageFormat>(), Ok(ImageFormat::Png));
        assert_eq!(
            "JPEG".parse::<ImageFormat>(),
            Ok(ImageFormat::Jpeg {
                quality: DEFAULT_JPEG_QUALITY
            })
        );
        assert_eq!(
            "jpg:60".parse::<ImageFormat>(),
            Ok(ImageFormat::Jpeg { quality: 60 })
        );
        assert_eq!(
            "jpeg:100".parse::<ImageFormat>(),
            Ok(ImageFormat::Jpeg { quality: 100 })
        );
        assert!("jpeg:0".parse::<ImageFormat>().is_err());
        assert!("jpeg:101".parse::<ImageFormat>().is_err());
        assert!("png:80".parse::<ImageFormat>().is_err());
        assert!("gif".parse::<ImageFormat>().is_err());
        assert_eq!(ImageFormat::default(), ImageFormat::Png);

        let jpeg = ImageFormat::Jpeg { quality: 70 }.encode(&sample()).expect("encode");
        assert!(jpeg.starts_with(&[0xFF, 0xD8]), "JPEG SOI marker");
    }

    #[test]
    fn default_recognize_image_writes_a_unique_temp_file_and_cleans_up() {
        let engine = FileEngine {
            seen: Mutex::new(Vec::new()),
        };
        let img = sample();

        engine
            .recognize_image(&ImageInput::from_pixels(&img), ImageFormat::Png)
            .expect("tile");

        // A whole-image call forwards the original bytes, extension included.
        engine
            .recognize_image(
                &ImageInput::with_source(&img, b"original-jpeg-bytes", "image/jpeg"),
                ImageFormat::Png,
            )
            .expect("whole image");

        let seen = engine.seen.lock().unwrap();
        assert_eq!(seen.len(), 2);
        assert_ne!(seen[0], seen[1], "temp names must be unique within a process");
        assert_eq!(seen[0].extension().and_then(|e| e.to_str()), Some("png"));
        assert_eq!(seen[1].extension().and_then(|e| e.to_str()), Some("jpg"));
        assert!(seen[0]
            .file_name()
            .unwrap()
            .to_string_lossy()
            .starts_with("ocr4code-input-"));
        assert!(seen[0].to_string_lossy().contains(&std::process::id().to_string()));
        assert!(!seen[0].exists(), "temp file must be removed after the call");
        assert!(!seen[1].exists(), "temp file must be removed after the call");
    }

    #[test]
    fn mime_is_inferred_the_same_way_as_before() {
        assert_eq!(mime_for_path(Path::new("a.png")), "image/png");
        assert_eq!(mime_for_path(Path::new("a.JPG")), "image/jpeg");
        assert_eq!(mime_for_path(Path::new("a.jpeg")), "image/jpeg");
        assert_eq!(mime_for_path(Path::new("a.webp")), "image/webp");
        assert_eq!(mime_for_path(Path::new("a.unknown")), "image/png");
    }
}
