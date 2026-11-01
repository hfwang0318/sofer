//! Deterministic engine for tests and for exercising the layout layer alone.
//!
//! It returns text with **no bounding boxes**, which is exactly the shape of a
//! VLM's output — so the geometry layer gets tested the same way it runs in
//! production.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use super::{Engine, EngineOutput, ImageFormat, ImageInput, RawLine};

pub struct Mock {
    text: Option<String>,
    text_file: Option<PathBuf>,
}

impl Mock {
    pub fn new(text: Option<String>, text_file: Option<PathBuf>) -> Result<Self> {
        Ok(Self { text, text_file })
    }

    /// The mock never looks at pixels, so it can answer without a file at all.
    fn replay(&self) -> Result<EngineOutput> {
        let text = if let Some(t) = &self.text {
            t.clone()
        } else if let Some(p) = &self.text_file {
            std::fs::read_to_string(p)
                .with_context(|| format!("reading mock text from {}", p.display()))?
        } else {
            anyhow::bail!("mock engine needs --mock-text or --mock-text-file")
        };
        let mut lines: Vec<RawLine> = text
            .lines()
            .map(|l| RawLine::plain(l.trim_end().to_string()))
            .collect();
        while lines.last().map(|l| l.text.is_empty()).unwrap_or(false) {
            lines.pop();
        }
        Ok(EngineOutput {
            engine: "mock".into(),
            lines,
            ms: 0,
        })
    }
}

impl Engine for Mock {
    fn name(&self) -> &str {
        "mock"
    }

    fn recognize(&self, _image: &Path) -> Result<EngineOutput> {
        self.replay()
    }

    fn recognize_image(
        &self,
        _input: &ImageInput<'_>,
        _format: ImageFormat,
    ) -> Result<EngineOutput> {
        self.replay()
    }
}
