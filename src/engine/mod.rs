//! Engine abstraction: anything that can turn an image into text lines.
//!
//! Engines differ wildly in what they give back (bbox or not, confidence or not),
//! so `RawLine` keeps those optional and `assemble` decides how to use them.

pub mod llamacpp;
pub mod mock;
pub mod tesseract;

use std::path::Path;

use anyhow::Result;

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

pub trait Engine: Send + Sync {
    fn name(&self) -> &str;
    fn recognize(&self, image: &Path) -> Result<EngineOutput>;

    /// Recognise, reporting incremental text through `on_delta` as it arrives.
    ///
    /// The default implementation is exactly one blocking [`Engine::recognize`]
    /// call followed by a single delta, so a streaming caller can treat every
    /// engine the same way without changing what it gets back.
    fn recognize_stream(
        &self,
        image: &Path,
        on_delta: &mut dyn FnMut(&str),
    ) -> Result<EngineOutput> {
        let out = self.recognize(image)?;
        if !out.lines.is_empty() {
            let text = out
                .lines
                .iter()
                .map(|line| line.text.as_str())
                .collect::<Vec<_>>()
                .join("\n");
            on_delta(&text);
        }
        Ok(out)
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
