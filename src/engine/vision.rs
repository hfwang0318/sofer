//! Apple Vision adapter — the macOS "fast" engine.
//!
//! `VNRecognizeTextRequest` (accurate, language correction off) returns one
//! observation per visual run of glyphs, so a single code line often arrives as
//! several fragments split at large gaps. This module re-joins fragments that
//! share a horizontal band and leaves indentation to the geometry layer, which is
//! the same contract every other engine follows (docs §4.3①).
//!
//! The recogniser itself lives in `vision_shim.swift`, compiled by `build.rs`
//! into `OUT_DIR`. Without a Swift toolchain the engine reports itself as
//! unavailable instead of failing the build.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Instant;

use anyhow::{Context, Result};
use serde::Deserialize;

use super::{Engine, EngineOutput, RawLine};
use crate::geom::Rect;

/// Two boxes belong to the same row when their vertical overlap covers at least
/// half of the shorter one. Vision's per-fragment boxes are tight around the
/// glyphs, so this is stable across ascenders/descenders and font sizes.
const ROW_OVERLAP_RATIO: f32 = 0.5;

/// Override the shim path (packaging, debugging a rebuilt shim by hand).
pub const SHIM_ENV: &str = "SOFER_VISION_SHIM";

pub struct Vision;

impl Vision {
    pub fn new() -> Self {
        Self
    }
}

impl Default for Vision {
    fn default() -> Self {
        Self::new()
    }
}

/// Path of the compiled shim, if this build has one.
///
/// `SOFER_VISION_SHIM` wins when set; otherwise the copy `build.rs` compiled
/// into this crate's `OUT_DIR` is used.
pub fn shim_path() -> Option<PathBuf> {
    if !cfg!(target_os = "macos") {
        return None;
    }
    if let Some(explicit) = std::env::var_os(SHIM_ENV) {
        let path = PathBuf::from(explicit);
        return path.is_file().then_some(path);
    }
    let path = Path::new(env!("OUT_DIR")).join("vision_ocr_shim");
    path.is_file().then_some(path)
}

pub fn available() -> bool {
    shim_path().is_some()
}

/// Human-readable reason for [`available`] returning `false`.
pub fn unavailable_reason() -> String {
    if !cfg!(target_os = "macos") {
        return format!(
            "Apple Vision is only available on macOS (this build targets {})",
            std::env::consts::OS
        );
    }
    format!(
        "Apple Vision shim was not compiled: install the Xcode command line tools \
         (for /usr/bin/swiftc) and rebuild, or point {SHIM_ENV} at a `vision_ocr_shim` binary"
    )
}

impl Engine for Vision {
    fn name(&self) -> &str {
        "vision"
    }

    fn recognize(&self, image: &Path) -> Result<EngineOutput> {
        let shim = shim_path().ok_or_else(|| anyhow::anyhow!("{}", unavailable_reason()))?;
        let started = Instant::now();
        let out = Command::new(&shim)
            .arg(image)
            .output()
            .with_context(|| format!("running Apple Vision shim ({})", shim.display()))?;
        if !out.status.success() {
            anyhow::bail!(
                "Apple Vision failed: {}",
                String::from_utf8_lossy(&out.stderr).trim()
            );
        }
        let json = String::from_utf8_lossy(&out.stdout);
        let (lines, shim_ms) = parse_json(&json)
            .with_context(|| format!("parsing Apple Vision output for {}", image.display()))?;
        Ok(EngineOutput {
            engine: "vision(accurate)".into(),
            lines,
            // The shim measures recognition only; fall back to wall time if the
            // field is missing so `--stats` never reports a bogus 0 ms.
            ms: shim_ms.unwrap_or_else(|| started.elapsed().as_millis() as u64),
        })
    }
}

#[derive(Deserialize)]
struct ShimOutput {
    lines: Vec<ShimLine>,
    #[serde(default)]
    ms: Option<f64>,
}

#[derive(Deserialize)]
struct ShimLine {
    text: String,
    #[serde(default)]
    conf: Option<f32>,
    bbox: [f32; 4],
}

/// Parse shim JSON into reading-order [`RawLine`]s, merging same-row fragments.
///
/// Returns the shim's self-reported elapsed time, if present.
pub fn parse_json(json: &str) -> Result<(Vec<RawLine>, Option<u64>)> {
    let output: ShimOutput = serde_json::from_str(json)?;
    let lines = output
        .lines
        .into_iter()
        .filter(|l| !l.text.trim().is_empty())
        .map(|l| {
            let [x0, y0, x1, y1] = l.bbox;
            RawLine::with_box(l.text, Rect::new(x0, y0, x1, y1), l.conf)
        })
        .collect();
    let ms = output.ms.map(|ms| ms.round().max(0.0) as u64);
    Ok((merge_fragments(lines), ms))
}

/// Group fragments into rows by vertical band, then join left-to-right.
///
/// Vision splits one line at large horizontal gaps (ASCII tables, soft wraps);
/// the geometry layer expects one [`RawLine`] per visual row, exactly like
/// tesseract's line grouping.
pub fn merge_fragments(lines: Vec<RawLine>) -> Vec<RawLine> {
    let mut items: Vec<RawLine> = lines
        .into_iter()
        .filter(|l| !l.text.trim().is_empty() && l.bbox.is_some())
        .collect();

    items.sort_by(|a, b| {
        let (a, b) = (a.bbox.unwrap(), b.bbox.unwrap());
        a.center_y()
            .partial_cmp(&b.center_y())
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(a.x0.partial_cmp(&b.x0).unwrap_or(std::cmp::Ordering::Equal))
    });

    let mut rows: Vec<Vec<RawLine>> = Vec::new();
    for line in items {
        match rows.last() {
            Some(row) if same_row(row, &line) => rows.last_mut().unwrap().push(line),
            _ => rows.push(vec![line]),
        }
    }
    rows.into_iter().map(merge_row).collect()
}

/// Whether `line`'s box overlaps the row's combined vertical extent enough to be
/// part of the same visual line.
fn same_row(row: &[RawLine], line: &RawLine) -> bool {
    let b = match line.bbox {
        Some(b) => b,
        None => return false,
    };
    let mut y0 = f32::INFINITY;
    let mut y1 = f32::NEG_INFINITY;
    for r in row {
        if let Some(a) = r.bbox {
            y0 = y0.min(a.y0);
            y1 = y1.max(a.y1);
        }
    }
    let row_h = y1 - y0;
    let line_h = b.y1 - b.y0;
    let overlap = y1.min(b.y1) - y0.max(b.y0);
    let shorter = row_h.min(line_h);
    shorter > 0.0 && overlap > 0.0 && overlap >= shorter * ROW_OVERLAP_RATIO
}

fn merge_row(mut row: Vec<RawLine>) -> RawLine {
    row.sort_by(|a, b| {
        let (a, b) = (a.bbox.unwrap(), b.bbox.unwrap());
        a.x0.partial_cmp(&b.x0).unwrap_or(std::cmp::Ordering::Equal)
    });

    let text = row
        .iter()
        .map(|l| l.text.trim())
        .filter(|t| !t.is_empty())
        .collect::<Vec<_>>()
        .join(" ");

    let mut x0 = f32::INFINITY;
    let mut y0 = f32::INFINITY;
    let mut x1 = f32::NEG_INFINITY;
    let mut y1 = f32::NEG_INFINITY;
    let mut confs: Vec<f32> = Vec::new();
    for l in &row {
        if let Some(b) = l.bbox {
            x0 = x0.min(b.x0);
            y0 = y0.min(b.y0);
            x1 = x1.max(b.x1);
            y1 = y1.max(b.y1);
        }
        if let Some(c) = l.conf {
            confs.push(c);
        }
    }
    let conf = if confs.is_empty() {
        None
    } else {
        Some(confs.iter().sum::<f32>() / confs.len() as f32)
    };
    RawLine::with_box(text, Rect::new(x0, y0, x1, y1), conf)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"{
        "lines": [
            {"text": "fn main() {", "conf": 0.9, "bbox": [120.0, 40.0, 260.0, 62.0]},
            {"text": "let x = 1;", "conf": 0.8, "bbox": [120.0, 80.0, 250.0, 102.0]},
            {"text": "// note", "conf": 0.7, "bbox": [270.0, 41.0, 340.0, 63.0]}
        ],
        "ms": 12.6
    }"#;

    #[test]
    fn parses_json_and_merges_same_row_fragments() {
        let (lines, ms) = parse_json(SAMPLE).unwrap();
        assert_eq!(ms, Some(13));
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].text, "fn main() { // note");
        assert_eq!(lines[1].text, "let x = 1;");
        let b = lines[0].bbox.unwrap();
        assert_eq!((b.x0, b.y0, b.x1, b.y1), (120.0, 40.0, 340.0, 63.0));
        let conf = lines[0].conf.unwrap();
        assert!((conf - 0.8).abs() < 1e-4);
    }

    #[test]
    fn fragments_merge_regardless_of_input_order() {
        let json = r#"{"lines": [
            {"text": "second", "conf": 0.5, "bbox": [200.0, 10.0, 260.0, 30.0]},
            {"text": "third",  "conf": 0.5, "bbox": [10.0, 60.0, 60.0, 80.0]},
            {"text": "first",  "conf": 0.5, "bbox": [10.0, 10.0, 190.0, 30.0]}
        ], "ms": 1.0}"#;
        let (lines, _) = parse_json(json).unwrap();
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].text, "first second");
        assert_eq!(lines[1].text, "third");
    }

    #[test]
    fn adjacent_rows_are_not_merged() {
        let json = r#"{"lines": [
            {"text": "top", "conf": 0.5, "bbox": [10.0, 10.0, 60.0, 30.0]},
            {"text": "bottom", "conf": 0.5, "bbox": [10.0, 32.0, 70.0, 52.0]}
        ], "ms": 1.0}"#;
        let (lines, _) = parse_json(json).unwrap();
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].text, "top");
        assert_eq!(lines[1].text, "bottom");
    }

    #[test]
    fn empty_text_and_missing_ms_are_tolerated() {
        let json = r#"{"lines": [{"text": "  ", "conf": 0.5, "bbox": [0.0, 0.0, 1.0, 1.0]}]}"#;
        let (lines, ms) = parse_json(json).unwrap();
        assert!(lines.is_empty());
        assert_eq!(ms, None);
    }

    #[test]
    fn malformed_json_is_an_error() {
        assert!(parse_json("not json").is_err());
    }

    #[cfg(not(target_os = "macos"))]
    #[test]
    fn vision_is_unavailable_off_macos() {
        assert!(!available());
        assert!(shim_path().is_none());
        let err = Vision::new()
            .recognize(Path::new("whatever.png"))
            .unwrap_err();
        assert!(err.to_string().contains("only available on macOS"));
    }
}
