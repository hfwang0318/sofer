//! Compile the Apple Vision shim when — and only when — targeting macOS.
//!
//! The shim is a single-file Swift program that links against the system Vision
//! framework, so no crates.io dependency is involved. When the Swift toolchain is
//! absent the build still succeeds; the engine then reports itself as unavailable
//! and `ocr4code doctor` explains why.

use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;

const SWIFTC: &str = "/usr/bin/swiftc";
const SHIM_SOURCE: &str = "src/engine/vision_shim.swift";
const SHIM_NAME: &str = "vision_ocr_shim";

fn main() {
    println!("cargo:rerun-if-changed={SHIM_SOURCE}");

    // `CARGO_CFG_TARGET_OS` is the *target*, not the host: cross-compiling for
    // Windows from macOS must skip the shim entirely.
    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("macos") {
        return;
    }
    let out_dir = match env::var("OUT_DIR") {
        Ok(dir) => PathBuf::from(dir),
        Err(_) => return,
    };
    build_shim(&out_dir);
}

fn build_shim(out_dir: &Path) {
    if !Path::new(SWIFTC).is_file() {
        println!(
            "cargo:warning=swiftc not found at {SWIFTC}; Apple Vision engine disabled \
             (install the Xcode command line tools to enable it)"
        );
        return;
    }
    let manifest_dir = match env::var("CARGO_MANIFEST_DIR") {
        Ok(dir) => PathBuf::from(dir),
        Err(_) => return,
    };
    let source = manifest_dir.join(SHIM_SOURCE);
    if !source.is_file() {
        println!(
            "cargo:warning=Vision shim source missing at {}; engine disabled",
            source.display()
        );
        return;
    }
    let target = out_dir.join(SHIM_NAME);

    // `-O` matters: the unoptimised shim spends noticeably longer in the
    // recognition pipeline on large screenshots.
    let output = Command::new(SWIFTC)
        .arg("-O")
        .arg("-o")
        .arg(&target)
        .arg(&source)
        .output();

    match output {
        Ok(result) if result.status.success() => {}
        Ok(result) => println!(
            "cargo:warning=compiling the Vision shim failed: {}",
            String::from_utf8_lossy(&result.stderr).trim()
        ),
        Err(e) => println!("cargo:warning=could not run {SWIFTC}: {e}"),
    }
}
