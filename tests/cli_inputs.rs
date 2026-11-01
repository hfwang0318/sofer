//! Integration tests for the CLI input layer: `-` on stdin, directory batches
//! and `--region` crops. They call the same helpers `main.rs` calls, plus one
//! end-to-end run through the real binary for the stdin path.

use std::io::{Cursor, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use image::{DynamicImage, ImageFormat, Rgb, RgbImage};
use ocr4code::cli_input::{
    expand_directory, is_supported_image, Region, TempImage, MAX_STDIN_BYTES,
};

// ---------------------------------------------------------------- helpers

/// Self-cleaning scratch directory, unique per test process and test case.
struct Scratch(PathBuf);

impl Scratch {
    fn new(tag: &str) -> Self {
        static SEQ: AtomicU64 = AtomicU64::new(0);
        let dir = std::env::temp_dir().join(format!(
            "ocr4code-cli-input-{}-{}-{tag}",
            std::process::id(),
            SEQ.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create scratch dir");
        Self(dir)
    }

    fn path(&self) -> &Path {
        &self.0
    }

    fn join(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn touch(path: &Path) {
    std::fs::write(path, b"placeholder").expect("write placeholder");
}

fn rgb_image(width: u32, height: u32) -> DynamicImage {
    DynamicImage::ImageRgb8(RgbImage::from_fn(width, height, |x, y| {
        Rgb([(x % 251) as u8, (y % 241) as u8, 17])
    }))
}

fn png_bytes(width: u32, height: u32) -> Vec<u8> {
    let mut out = Vec::new();
    rgb_image(width, height)
        .write_to(&mut Cursor::new(&mut out), ImageFormat::Png)
        .expect("encode png");
    out
}

fn write_image(path: &Path, width: u32, height: u32, format: ImageFormat) {
    rgb_image(width, height)
        .save_with_format(path, format)
        .expect("save fixture");
}

// ---------------------------------------------------------------- region

#[test]
fn region_parses_four_positive_integers() {
    assert_eq!(
        Region::parse("10,20,300,400").unwrap(),
        Region {
            x: 10,
            y: 20,
            width: 300,
            height: 400
        }
    );
    assert_eq!(
        Region::parse(" 0 , 1 , 2 , 3 ").unwrap(),
        Region {
            x: 0,
            y: 1,
            width: 2,
            height: 3
        }
    );
    assert_eq!(Region::parse("0,0,1,1").unwrap().to_string(), "0,0,1,1");
}

#[test]
fn region_rejects_malformed_input() {
    let bad = [
        "",
        "1,2,3",
        "1,2,3,4,5",
        "a,0,10,10",
        "-1,0,10,10",
        "1.5,0,10,10",
        "0,0,0,10",
        "0,0,10,0",
    ];
    for input in bad {
        let err = Region::parse(input).expect_err(input);
        let msg = format!("{err:#}");
        assert!(msg.contains("--region"), "{input:?}: {msg}");
    }
}

#[test]
fn region_bounds_name_the_real_image_size() {
    let image_w = 100;
    let image_h = 50;

    Region::parse("10,10,90,40")
        .unwrap()
        .validate(image_w, image_h)
        .expect("inside the image, including the far edge");
    Region::parse("0,0,100,50")
        .unwrap()
        .validate(image_w, image_h)
        .expect("the whole image");

    let err = Region::parse("10,10,91,40")
        .unwrap()
        .validate(image_w, image_h)
        .unwrap_err();
    let msg = format!("{err:#}");
    assert!(msg.contains("100x50"), "must report the real size: {msg}");
    assert!(msg.contains("91"), "must quote the bad region: {msg}");

    let err = Region::parse("0,0,10,51")
        .unwrap()
        .validate(image_w, image_h)
        .unwrap_err();
    assert!(format!("{err:#}").contains("100x50"));

    // x + width overflows u32: still a user error, not a panic.
    let err = Region::parse(&format!("{},{},2,2", u32::MAX, u32::MAX))
        .unwrap()
        .validate(image_w, image_h)
        .unwrap_err();
    assert!(format!("{err:#}").contains("overflow"));
}

// ---------------------------------------------------------------- directories

#[test]
fn directory_expansion_filters_extensions_and_sorts() {
    let dir = Scratch::new("dirs");
    for name in ["b.png", "a.JPG", "c.webp", "z.jpeg", "notes.txt", "d.gif"] {
        touch(&dir.join(name));
    }
    let nested = dir.join("sub");
    std::fs::create_dir_all(&nested).unwrap();
    touch(&nested.join("e.BMP"));
    touch(&nested.join("f.tiff"));
    touch(&nested.join("g.md"));

    let flat = expand_directory(dir.path(), false).unwrap();
    let flat_names: Vec<String> = flat
        .iter()
        .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
        .collect();
    assert_eq!(flat_names, ["a.JPG", "b.png", "c.webp", "z.jpeg"]);

    let deep = expand_directory(dir.path(), true).unwrap();
    let deep_names: Vec<String> = deep
        .iter()
        .map(|p| {
            p.strip_prefix(dir.path())
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/")
        })
        .collect();
    // Recursive batches are ordered by full path, so a subdirectory lands at its
    // own alphabetical position ("sub/" before "z.jpeg") rather than after it.
    assert_eq!(
        deep_names,
        [
            "a.JPG",
            "b.png",
            "c.webp",
            "sub/e.BMP",
            "sub/f.tiff",
            "z.jpeg"
        ]
    );

    assert!(is_supported_image(Path::new("x.PNG")));
    assert!(is_supported_image(Path::new("x.jpeg")));
    assert!(!is_supported_image(Path::new("x.gif")));
    assert!(!is_supported_image(Path::new("x")));
}

#[test]
fn empty_directory_is_an_explicit_error() {
    let dir = Scratch::new("empty");
    touch(&dir.join("readme.md"));

    let err = expand_directory(dir.path(), false).unwrap_err();
    let msg = format!("{err:#}");
    assert!(msg.contains("no supported images"), "{msg}");
    assert!(msg.contains("png/jpg/jpeg/webp/bmp/tiff"), "{msg}");
    assert!(msg.contains("--recursive"), "{msg}");

    let err = expand_directory(dir.path(), true).unwrap_err();
    let msg = format!("{err:#}");
    assert!(msg.contains("no supported images"), "{msg}");
    assert!(!msg.contains("--recursive"), "{msg}");
}

// ---------------------------------------------------------------- stdin

#[test]
fn stdin_spools_to_unique_temp_files_and_cleans_up() {
    let bytes = png_bytes(4, 4);
    let first = TempImage::read_stdin(Cursor::new(bytes.clone())).unwrap();
    let second = TempImage::read_stdin(Cursor::new(bytes)).unwrap();

    assert_ne!(first.path(), second.path(), "temp names must be unique");
    assert_eq!(
        first.path().extension().and_then(|e| e.to_str()),
        Some("png"),
        "the extension must follow the sniffed format"
    );
    let name = first
        .path()
        .file_name()
        .unwrap()
        .to_string_lossy()
        .into_owned();
    assert!(
        name.contains(&std::process::id().to_string()),
        "temp name must carry the pid: {name}"
    );

    let (a, b) = (first.path().to_path_buf(), second.path().to_path_buf());
    assert!(a.is_file() && b.is_file());
    drop(first);
    assert!(!a.exists(), "temp file survived a successful drop");
    drop(second);
    assert!(!b.exists());
}

#[test]
fn stdin_rejects_empty_unknown_and_oversized_input() {
    let err = TempImage::read_stdin(Cursor::new(Vec::<u8>::new())).unwrap_err();
    let msg = format!("{err:#}");
    assert!(msg.contains("stdin is empty"), "{msg}");
    assert!(
        msg.contains("ocr4code ocr -"),
        "needs an actionable hint: {msg}"
    );

    let err = TempImage::read_stdin(Cursor::new(b"definitely not an image".to_vec())).unwrap_err();
    let msg = format!("{err:#}");
    assert!(msg.contains("cannot detect the image format"), "{msg}");

    /// Produces zeroes without allocating a 64 MiB fixture up front.
    struct Endless;
    impl Read for Endless {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            buf.fill(0);
            Ok(buf.len())
        }
    }

    let err = TempImage::read_stdin(Endless).unwrap_err();
    let msg = format!("{err:#}");
    assert!(msg.contains("too large"), "{msg}");
    assert!(
        msg.contains(&format!("{} MiB", MAX_STDIN_BYTES / (1024 * 1024))),
        "must name the limit: {msg}"
    );
}

#[test]
fn failed_decode_still_removes_the_temp_file() {
    // The temp file is spooled first and only decoded later by the pipeline. If
    // that decode fails, the guard must still delete what it created.
    let temp = TempImage::read_stdin(Cursor::new(png_bytes(8, 8))).expect("valid png");
    let path = temp.path().to_path_buf();
    assert!(path.is_file());

    std::fs::write(&path, b"no longer a decodable image").unwrap();
    assert!(image::open(&path).is_err(), "the fixture must not decode");
    drop(temp);
    assert!(!path.exists(), "the error path must clean up the temp file");
}

// ---------------------------------------------------------------- crops

#[test]
fn crop_keeps_the_source_format_and_reports_bounds() {
    let dir = Scratch::new("crop");

    let png = dir.join("shot.png");
    write_image(&png, 40, 30, ImageFormat::Png);
    let temp = TempImage::crop(&png, Region::parse("5,5,10,12").unwrap()).unwrap();
    assert_eq!(
        temp.path().extension().and_then(|e| e.to_str()),
        Some("png"),
        "the crop must keep the original mime"
    );
    let cropped = image::open(temp.path()).unwrap();
    assert_eq!((cropped.width(), cropped.height()), (10, 12));
    let cropped_path = temp.path().to_path_buf();
    drop(temp);
    assert!(!cropped_path.exists());

    let jpg = dir.join("shot.jpg");
    write_image(&jpg, 40, 30, ImageFormat::Jpeg);
    let temp = TempImage::crop(&jpg, Region::parse("0,0,40,30").unwrap()).unwrap();
    assert_eq!(
        temp.path().extension().and_then(|e| e.to_str()),
        Some("jpg"),
        "a jpeg crop stays a jpeg"
    );

    let err = TempImage::crop(&png, Region::parse("35,0,10,10").unwrap()).unwrap_err();
    let msg = format!("{err:#}");
    assert!(msg.contains("40x30"), "must report the real size: {msg}");
}

// ---------------------------------------------------------------- end to end

#[test]
fn stdin_pipe_runs_the_real_cli() {
    let image = std::fs::read("bench/images/editor-rust@1x.png").expect("bench fixture");
    let mut child = std::process::Command::new(env!("CARGO_BIN_EXE_ocr4code"))
        .args([
            "ocr",
            "-",
            "--engine",
            "mock",
            "--mock-text-file",
            "bench/truth/editor-rust.txt",
        ])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("spawn ocr4code");

    child
        .stdin
        .take()
        .expect("stdin pipe")
        .write_all(&image)
        .expect("feed the image");

    let out = child.wait_with_output().expect("wait for ocr4code");
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let text = String::from_utf8(out.stdout).expect("utf8 output");
    assert!(
        text.contains("use std::collections::HashMap;"),
        "stdin output: {text}"
    );
}
