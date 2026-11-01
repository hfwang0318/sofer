//! llama.cpp adapter — the primary engine (GLM-OCR Q8 GGUF, MIT).
//!
//! Talks to a `llama-server` over its OpenAI-compatible endpoint. The model file is
//! identical on macOS and Windows; only the backend (Metal / Vulkan / CUDA / CPU)
//! differs, which is why this is the one engine that ships as "the" engine.

use std::path::Path;
use std::time::Instant;

use anyhow::{Context, Result};
use base64::Engine as _;
use serde_json::{json, Value};

use super::{mime_for_path, Engine, EngineOutput, ImageFormat, ImageInput, RawLine};

pub const DEFAULT_URL: &str = "http://127.0.0.1:8099";
/// The prompt matters more than any other knob. Measured on bench/wide-table:
/// `OCR:` makes the model emit one *cell* per line (row structure destroyed),
/// while this wording keeps rows and columns intact and is also faster
/// (~0.9 s vs ~1.3 s) because it produces fewer tokens.
pub const DEFAULT_PROMPT: &str =
    "OCR: preserve the original layout, spacing and line breaks exactly.";

pub fn default_url() -> String {
    std::env::var("OCR4CODE_LLAMA_URL").unwrap_or_else(|_| DEFAULT_URL.to_string())
}

pub struct LlamaCpp {
    url: String,
    prompt: String,
    max_tokens: u32,
}

impl LlamaCpp {
    pub fn new(url: String, prompt: String, max_tokens: u32) -> Self {
        Self {
            url,
            prompt,
            max_tokens,
        }
    }

    pub fn health(&self) -> bool {
        ureq::get(&format!("{}/health", self.url))
            .timeout(std::time::Duration::from_millis(800))
            .call()
            .map(|r| r.status() == 200)
            .unwrap_or(false)
    }

    /// One chat completion for already-encoded image bytes.
    fn send(&self, t0: Instant, bytes: &[u8], mime: &str) -> Result<EngineOutput> {
        let b64 = base64::engine::general_purpose::STANDARD.encode(bytes);
        let payload = json!({
            "messages": [{
                "role": "user",
                "content": [
                    {"type": "image_url", "image_url": {"url": format!("data:{mime};base64,{b64}")}},
                    {"type": "text", "text": self.prompt}
                ]
            }],
            "temperature": 0.0,
            "max_tokens": self.max_tokens,
            "stream": false
        });

        let resp: Value = ureq::post(&format!("{}/v1/chat/completions", self.url))
            .timeout(std::time::Duration::from_secs(180))
            .send_json(payload)
            .map_err(|e| {
                anyhow::anyhow!(
                    "llama-server at {} not reachable ({e}); start it with `ocr4code doctor --hint`",
                    self.url
                )
            })?
            .into_json()
            .context("decoding llama-server response")?;

        let text = resp["choices"][0]["message"]["content"]
            .as_str()
            .unwrap_or_default()
            .to_string();

        let mut lines: Vec<RawLine> = text
            .lines()
            .map(|l| RawLine::plain(l.trim_end().to_string()))
            .collect();
        while lines.last().map(|l| l.text.trim().is_empty()).unwrap_or(false) {
            lines.pop();
        }

        Ok(EngineOutput {
            engine: format!("llamacpp({})", self.url),
            lines,
            ms: t0.elapsed().as_millis() as u64,
        })
    }
}

impl Engine for LlamaCpp {
    fn name(&self) -> &str {
        "llamacpp"
    }

    fn recognize(&self, image: &Path) -> Result<EngineOutput> {
        let t0 = Instant::now();
        let bytes = std::fs::read(image).with_context(|| format!("reading {}", image.display()))?;
        self.send(t0, &bytes, mime_for_path(image))
    }

    /// Encode in memory and post — no temp file, no second read of the source.
    fn recognize_image(&self, input: &ImageInput<'_>, format: ImageFormat) -> Result<EngineOutput> {
        let t0 = Instant::now();
        // PNG (the default) forwards the caller's original bytes untouched, so the
        // request stays byte-identical to the path-based flow. An explicit JPEG
        // request re-encodes from the decoded pixels.
        if let (ImageFormat::Png, Some((bytes, mime))) = (format, input.source) {
            return self.send(t0, bytes, mime);
        }
        let bytes = format.encode(input.image)?;
        self.send(t0, &bytes, format.mime())
    }
}
