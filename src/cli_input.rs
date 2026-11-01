//! CLI input plumbing: stdin, directories and `--region` crops.
//!
//! [`crate::pipeline::recognize`] only understands a file path, so every other
//! shape the CLI accepts — `-` on stdin, a directory to batch over, a pixel
//! rectangle to crop — is normalised here into a real image file first. Nothing
//! in this module changes what the kernel sees: a crop is still a whole image,
//! and the engine output is still assembled from pixels by the pipeline.
//!
//! Temp files are named with the process id (plus a per-process sequence and a
//! timestamp) and remove themselves on drop, which covers the success path and
//! the error path alike.

use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{bail, Context, Result};
use image::{DynamicImage, ImageFormat, ImageReader};

/// Highest image size accepted on stdin. Bigger than any real screenshot, small
/// enough that a wrong pipe (`cat huge.log | ocr4code ocr -`) fails fast.
pub const MAX_STDIN_BYTES: u64 = 64 * 1024 * 1024;

/// Extensions the CLI accepts and that a directory is expanded to.
pub const SUPPORTED_EXTENSIONS: &[&str] = &["png", "jpg", "jpeg", "webp", "bmp", "tiff"];

/// Human-readable form of [`SUPPORTED_EXTENSIONS`], used in error messages.
const SUPPORTED_HINT: &str = "png/jpg/jpeg/webp/bmp/tiff";

/// `-` means "read the image from stdin".
pub fn is_stdin(arg: &Path) -> bool {
    arg.as_os_str() == "-"
}

/// A pixel rectangle: `--region X,Y,W,H`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Region {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

impl Region {
    /// Parse `X,Y,W,H`. Four non-negative integers, no zero-sized sides.
    pub fn parse(s: &str) -> Result<Self> {
        let parts: Vec<&str> = s.split(',').map(str::trim).collect();
        if parts.len() != 4 {
            bail!(
                "--region expects X,Y,W,H (four integers, e.g. `--region 0,0,800,600`), got `{s}`"
            );
        }
        const NAMES: [&str; 4] = ["x", "y", "width", "height"];
        let mut n = [0u32; 4];
        for (i, (name, part)) in NAMES.iter().zip(&parts).enumerate() {
            n[i] = part.parse::<u32>().with_context(|| {
                format!("--region {name} `{part}` is not a non-negative integer")
            })?;
        }
        let region = Self {
            x: n[0],
            y: n[1],
            width: n[2],
            height: n[3],
        };
        if region.width == 0 || region.height == 0 {
            bail!("--region width and height must be greater than zero, got `{s}`");
        }
        Ok(region)
    }

    /// Check the rectangle against a decoded image, naming the real size when it
    /// does not fit — that is the only way a user can pick a valid one.
    pub fn validate(&self, width: u32, height: u32) -> Result<()> {
        let x1 = self
            .x
            .checked_add(self.width)
            .context("--region x + width overflows u32")?;
        let y1 = self
            .y
            .checked_add(self.height)
            .context("--region y + height overflows u32")?;
        if x1 > width || y1 > height {
            bail!(
                "--region {} is outside the {width}x{height} image: \
                 it needs x+width <= {width} and y+height <= {height}",
                self
            );
        }
        Ok(())
    }
}

impl std::fmt::Display for Region {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{},{},{},{}", self.x, self.y, self.width, self.height)
    }
}

/// A process-unique temp image that deletes itself when dropped.
///
/// Used for stdin (bytes have to land somewhere `recognize` can open) and for
/// `--region` crops. Both the success and the error path drop the guard, so the
/// file never outlives the run.
#[derive(Debug)]
pub struct TempImage {
    path: PathBuf,
}

impl TempImage {
    /// Spool one image from `reader` (stdin in the CLI) to a temp file.
    ///
    /// The extension comes from the file's content, not from the caller, so
    /// `image::open` picks the right decoder later.
    pub fn read_stdin(mut reader: impl Read) -> Result<Self> {
        let mut bytes = Vec::new();
        reader
            .by_ref()
            .take(MAX_STDIN_BYTES + 1)
            .read_to_end(&mut bytes)
            .context("reading stdin")?;

        if bytes.is_empty() {
            bail!(
                "stdin is empty: pipe an image into `ocr4code ocr -`, \
                 e.g. `cat shot.png | ocr4code ocr -`"
            );
        }
        if bytes.len() as u64 > MAX_STDIN_BYTES {
            bail!(
                "stdin image is too large ({:.1} MiB, limit {} MiB): \
                 crop or resize it first, e.g. `sips --resampleWidth 2560 shot.png`, \
                 or pass a file path instead",
                bytes.len() as f64 / (1024.0 * 1024.0),
                MAX_STDIN_BYTES / (1024 * 1024)
            );
        }

        let sniffed = ImageReader::new(std::io::Cursor::new(bytes.as_slice()))
            .with_guessed_format()
            .context("reading the image header from stdin")?;
        let format = sniffed.format().with_context(|| {
            format!(
                "cannot detect the image format on stdin (expected {SUPPORTED_HINT}): \
                 check that the pipe carries an image, e.g. `cat shot.png | ocr4code ocr -`"
            )
        })?;
        let ext = extension_for(format).with_context(|| {
            format!(
                "stdin carries {}, which is not supported ({SUPPORTED_HINT})",
                format.to_mime_type()
            )
        })?;
        // Header-only decode: a truncated stream fails here with a message about
        // stdin instead of a temp path the user has never seen.
        sniffed.into_dimensions().with_context(|| {
            format!("decoding stdin image ({ext}): the stream looks truncated or corrupt")
        })?;

        let (path, mut file) = create_temp(ext)?;
        let guard = Self { path };
        file.write_all(&bytes)
            .with_context(|| format!("writing {}", guard.path.display()))?;
        file.flush()
            .with_context(|| format!("writing {}", guard.path.display()))?;
        Ok(guard)
    }

    /// Crop `path` to `region` and save the crop in the source's own format, so
    /// the engine receives the same MIME type as the original file.
    pub fn crop(path: &Path, region: Region) -> Result<Self> {
        let (format, ext) = detect_format(path)?;
        let img = image::open(path).with_context(|| format!("opening image {}", path.display()))?;
        // The CLI prefixes per-image errors with the label, so no path context
        // here — it would print the file name twice.
        region.validate(img.width(), img.height())?;
        let cropped = img.crop_imm(region.x, region.y, region.width, region.height);
        Self::save(&cropped, format, ext)
    }

    fn save(img: &DynamicImage, format: ImageFormat, ext: &str) -> Result<Self> {
        let (path, mut file) = create_temp(ext)?;
        let guard = Self { path };
        img.write_to(&mut file, format)
            .with_context(|| format!("writing cropped temp image {}", guard.path.display()))?;
        file.flush()
            .with_context(|| format!("writing cropped temp image {}", guard.path.display()))?;
        Ok(guard)
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempImage {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

/// Expand a directory argument into its image files, sorted by path.
///
/// Unsupported files are skipped; a directory with no images is an error,
/// because silently printing nothing would look like a successful run.
pub fn expand_directory(dir: &Path, recursive: bool) -> Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    collect_images(dir, recursive, &mut out)?;
    // Lexicographic, so a batch is reproducible across runs and platforms.
    out.sort();
    if out.is_empty() {
        let hint = if recursive {
            ""
        } else {
            "; pass files explicitly or add --recursive to search subdirectories"
        };
        bail!(
            "no supported images ({SUPPORTED_HINT}) in directory {}{hint}",
            dir.display()
        );
    }
    Ok(out)
}

/// Does the path carry one of [`SUPPORTED_EXTENSIONS`] (case-insensitive)?
pub fn is_supported_image(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| {
            let lower = e.to_ascii_lowercase();
            SUPPORTED_EXTENSIONS.contains(&lower.as_str())
        })
        .unwrap_or(false)
}

fn collect_images(dir: &Path, recursive: bool, out: &mut Vec<PathBuf>) -> Result<()> {
    let entries =
        std::fs::read_dir(dir).with_context(|| format!("reading directory {}", dir.display()))?;
    for entry in entries {
        let entry = entry.with_context(|| format!("reading directory {}", dir.display()))?;
        let path = entry.path();
        let file_type = entry
            .file_type()
            .with_context(|| format!("inspecting {}", path.display()))?;
        if file_type.is_dir() {
            if recursive {
                collect_images(&path, recursive, out)?;
            }
        } else if is_supported_image(&path) && (file_type.is_file() || path.is_file()) {
            // `path.is_file()` follows symlinks, so a symlinked image counts.
            // Symlinked *directories* are never descended into: that is how a
            // recursive walk turns into an infinite loop.
            out.push(path);
        }
    }
    Ok(())
}

/// Content-based format of an existing file, restricted to what the CLI supports.
fn detect_format(path: &Path) -> Result<(ImageFormat, &'static str)> {
    let reader = ImageReader::open(path)
        .with_context(|| format!("opening image {}", path.display()))?
        .with_guessed_format()
        .with_context(|| format!("reading the header of {}", path.display()))?;
    let format = reader.format().with_context(|| {
        format!(
            "cannot detect the image format of {} (expected {SUPPORTED_HINT})",
            path.display()
        )
    })?;
    let ext = extension_for(format).with_context(|| {
        format!(
            "{} is {}, which is not supported ({SUPPORTED_HINT})",
            path.display(),
            format.to_mime_type()
        )
    })?;
    Ok((format, ext))
}

/// The canonical extension of a format we can both decode and encode.
fn extension_for(format: ImageFormat) -> Option<&'static str> {
    match format {
        ImageFormat::Png => Some("png"),
        ImageFormat::Jpeg => Some("jpg"),
        ImageFormat::WebP => Some("webp"),
        ImageFormat::Bmp => Some("bmp"),
        ImageFormat::Tiff => Some("tiff"),
        _ => None,
    }
}

/// Create a fresh temp file with a process-unique name and `ext`.
///
/// `create_new` makes the name safe even when several `ocr4code` processes
/// share the same temp directory; the loop retries on the (unlikely) collision.
fn create_temp(ext: &str) -> Result<(PathBuf, File)> {
    for _ in 0..16 {
        let path = temp_path(ext);
        match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(file) => return Ok((path, file)),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => {
                return Err(e).with_context(|| format!("creating temp file {}", path.display()))
            }
        }
    }
    bail!(
        "could not create a unique temp file in {}",
        std::env::temp_dir().display()
    )
}

fn temp_path(ext: &str) -> PathBuf {
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let seq = SEQ.fetch_add(1, Ordering::Relaxed);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(0);
    std::env::temp_dir().join(format!(
        "ocr4code-input-{}-{seq}-{nanos}.{ext}",
        std::process::id()
    ))
}
