//! Tesseract adapter (TSV output → lines with word-level boxes).
//!
//! Kept as the "no GPU / low-end Windows" fallback: 40–60 MB and ~0.5 s per image,
//! at the cost of symbol fidelity (see docs §4.5).

use std::path::Path;
use std::process::Command;
use std::time::Instant;

use anyhow::{Context, Result};

use super::{Engine, EngineOutput, RawLine};
use crate::geom::Rect;

pub struct Tesseract {
    lang: String,
    psm: u32,
}

impl Tesseract {
    pub fn new(lang: String, psm: u32) -> Self {
        Self { lang, psm }
    }
}

pub fn available() -> bool {
    Command::new("tesseract")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

impl Engine for Tesseract {
    fn name(&self) -> &str {
        "tesseract"
    }

    fn recognize(&self, image: &Path) -> Result<EngineOutput> {
        let t0 = Instant::now();
        let out = Command::new("tesseract")
            .arg(image)
            .arg("stdout")
            .args(["-l", &self.lang, "--psm", &self.psm.to_string(), "tsv"])
            .output()
            .context("running tesseract (is it on PATH?)")?;
        if !out.status.success() {
            anyhow::bail!(
                "tesseract failed: {}",
                String::from_utf8_lossy(&out.stderr).trim()
            );
        }
        let tsv = String::from_utf8_lossy(&out.stdout);
        let lines = parse_tsv(&tsv);
        Ok(EngineOutput {
            engine: format!("tesseract(-l {}, --psm {})", self.lang, self.psm),
            lines,
            ms: t0.elapsed().as_millis() as u64,
        })
    }
}

/// Group word rows of `tesseract ... tsv` into lines keyed by (block, par, line).
pub fn parse_tsv(tsv: &str) -> Vec<RawLine> {
    struct Acc {
        words: Vec<(f32, String)>,
        x0: f32,
        y0: f32,
        x1: f32,
        y1: f32,
        conf: Vec<f32>,
    }
    let mut rows: std::collections::BTreeMap<(u32, u32, u32), Acc> =
        std::collections::BTreeMap::new();

    for l in tsv.lines().skip(1) {
        let f: Vec<&str> = l.split('\t').collect();
        if f.len() < 12 || f[0] != "5" {
            continue;
        }
        let text = f[11].trim();
        if text.is_empty() {
            continue;
        }
        let (Ok(x), Ok(y), Ok(w), Ok(h)) = (
            f[6].parse::<f32>(),
            f[7].parse::<f32>(),
            f[8].parse::<f32>(),
            f[9].parse::<f32>(),
        ) else {
            continue;
        };
        let conf = f[10].parse::<f32>().unwrap_or(-1.0);
        let key = (
            f[2].parse().unwrap_or(0),
            f[3].parse().unwrap_or(0),
            f[4].parse().unwrap_or(0),
        );
        let e = rows.entry(key).or_insert(Acc {
            words: Vec::new(),
            x0: x,
            y0: y,
            x1: x + w,
            y1: y + h,
            conf: Vec::new(),
        });
        e.words.push((x, text.to_string()));
        e.x0 = e.x0.min(x);
        e.y0 = e.y0.min(y);
        e.x1 = e.x1.max(x + w);
        e.y1 = e.y1.max(y + h);
        if conf >= 0.0 {
            e.conf.push(conf / 100.0);
        }
    }

    rows.into_values()
        .map(|mut e| {
            e.words.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
            let text = e
                .words
                .iter()
                .map(|(_, t)| t.as_str())
                .collect::<Vec<_>>()
                .join(" ");
            let conf = if e.conf.is_empty() {
                None
            } else {
                Some(e.conf.iter().sum::<f32>() / e.conf.len() as f32)
            };
            RawLine::with_box(text, Rect::new(e.x0, e.y0, e.x1, e.y1), conf)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_two_words_into_one_line() {
        let tsv = "level\tpage_num\tblock_num\tpar_num\tline_num\tword_num\tleft\ttop\twidth\theight\tconf\ttext\n\
5\t1\t1\t1\t1\t1\t10\t20\t40\t12\t96.5\tlet\n\
5\t1\t1\t1\t1\t2\t55\t20\t30\t12\t91.0\tx\n";
        let lines = parse_tsv(tsv);
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].text, "let x");
        let b = lines[0].bbox.unwrap();
        assert_eq!((b.x0, b.y0, b.x1), (10.0, 20.0, 85.0));
    }
}
