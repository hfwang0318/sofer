//! llama.cpp adapter — the primary engine (GLM-OCR Q8 GGUF, MIT).
//!
//! Talks to a `llama-server` over its OpenAI-compatible endpoint. The model file is
//! identical on macOS and Windows; only the backend (Metal / Vulkan / CUDA / CPU)
//! differs, which is why this is the one engine that ships as "the" engine.

use std::io::Read;
use std::path::Path;
use std::time::{Duration, Instant};

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

const REQUEST_TIMEOUT: Duration = Duration::from_secs(180);

pub fn default_url() -> String {
    std::env::var("SOFER_LLAMA_URL").unwrap_or_else(|_| DEFAULT_URL.to_string())
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

    fn endpoint(&self) -> String {
        format!("{}/v1/chat/completions", self.url)
    }

    /// The OpenAI-compatible body both paths share. `stream` only changes how
    /// the reply is framed, not how the model is asked.
    fn payload(&self, bytes: &[u8], mime: &str, stream: bool) -> Value {
        let b64 = base64::engine::general_purpose::STANDARD.encode(bytes);
        json!({
            "messages": [{
                "role": "user",
                "content": [
                    {"type": "image_url", "image_url": {"url": format!("data:{mime};base64,{b64}")}},
                    {"type": "text", "text": self.prompt}
                ]
            }],
            "temperature": 0.0,
            "max_tokens": self.max_tokens,
            "stream": stream
        })
    }

    /// One chat completion for already-encoded image bytes.
    fn send(&self, t0: Instant, bytes: &[u8], mime: &str) -> Result<EngineOutput> {
        let resp: Value = ureq::post(&self.endpoint())
            .timeout(REQUEST_TIMEOUT)
            .send_json(self.payload(bytes, mime, false))
            .map_err(|e| self.unreachable(e))?
            .into_json()
            .context("decoding llama-server response")?;
        let text = resp["choices"][0]["message"]["content"]
            .as_str()
            .unwrap_or_default()
            .to_string();
        Ok(self.output(t0, &text))
    }

    fn unreachable(&self, error: impl std::fmt::Display) -> anyhow::Error {
        anyhow::anyhow!(
            "llama-server at {} not reachable ({error}); start it with `sofer doctor --hint`",
            self.url
        )
    }

    /// Same line shaping as the streaming path, so both produce the same rows
    /// for the same text.
    fn output(&self, started: Instant, text: &str) -> EngineOutput {
        let mut lines: Vec<RawLine> = text
            .lines()
            .map(|l| RawLine::plain(l.trim_end().to_string()))
            .collect();
        while lines
            .last()
            .map(|l| l.text.trim().is_empty())
            .unwrap_or(false)
        {
            lines.pop();
        }
        EngineOutput {
            engine: format!("llamacpp({})", self.url),
            lines,
            ms: started.elapsed().as_millis() as u64,
        }
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

    /// Server-sent events: `data: {"choices":[{"delta":{"content":"…"}}]}` per
    /// token, terminated by `data: [DONE]`. A reply that ends before `[DONE]`
    /// is treated as a broken connection, never as a short answer.
    fn recognize_stream(
        &self,
        image: &Path,
        on_delta: &mut dyn FnMut(&str),
    ) -> Result<EngineOutput> {
        let t0 = Instant::now();
        let bytes = std::fs::read(image).with_context(|| format!("reading {}", image.display()))?;
        let payload = self.payload(&bytes, mime_for_path(image), true);
        let mut reader = ureq::post(&self.endpoint())
            .timeout(REQUEST_TIMEOUT)
            .send_json(payload)
            .map_err(|e| self.unreachable(e))?
            .into_reader();

        let mut parser = SseParser::new();
        let mut text = String::new();
        let mut buf = [0u8; 8192];
        loop {
            let read = reader
                .read(&mut buf)
                .context("reading llama-server stream")?;
            if read == 0 {
                break;
            }
            for delta in parser.push(&buf[..read]) {
                text.push_str(&delta);
                on_delta(&delta);
            }
            if parser.is_done() {
                break;
            }
        }
        for delta in parser.finish() {
            text.push_str(&delta);
            on_delta(&delta);
        }
        if !parser.is_done() {
            // A 200 that never sent an SSE frame is not a short answer; it is a
            // response we cannot trust, so let the caller retry without streaming.
            if text.is_empty() {
                anyhow::bail!("llama-server returned no SSE data");
            }
            anyhow::bail!(
                "llama-server stream ended before [DONE] after {} chars",
                text.chars().count()
            );
        }
        Ok(self.output(t0, &text))
    }
}

/// Incremental parser for the OpenAI-style SSE stream.
///
/// Bytes are buffered until a full line is available, so a chunk boundary in the
/// middle of a line — or in the middle of a multi-byte UTF-8 character — cannot
/// corrupt a delta. Blank lines, comments (`: keep-alive`) and other event
/// fields are ignored; malformed JSON is skipped instead of failing the whole
/// reply, because a preview must never turn a working one-shot path into an
/// error.
#[derive(Debug, Default)]
struct SseParser {
    buffer: Vec<u8>,
    done: bool,
}

impl SseParser {
    pub fn new() -> Self {
        Self::default()
    }

    /// True once a `data: [DONE]` line has been seen.
    pub fn is_done(&self) -> bool {
        self.done
    }

    /// Feed one network chunk; returns the deltas that are complete so far.
    pub fn push(&mut self, chunk: &[u8]) -> Vec<String> {
        self.buffer.extend_from_slice(chunk);
        let mut deltas = Vec::new();
        let mut start = 0;
        while let Some(offset) = self.buffer[start..].iter().position(|byte| *byte == b'\n') {
            let end = start + offset;
            handle_line(&self.buffer[start..end], &mut self.done, &mut deltas);
            start = end + 1;
            if self.done {
                break;
            }
        }
        self.buffer.drain(..start);
        if self.done {
            self.buffer.clear();
        }
        deltas
    }

    /// Flush a trailing line that never got its newline before EOF.
    pub fn finish(&mut self) -> Vec<String> {
        let mut deltas = Vec::new();
        if !self.done && !self.buffer.is_empty() {
            let line = std::mem::take(&mut self.buffer);
            handle_line(&line, &mut self.done, &mut deltas);
        }
        self.buffer.clear();
        deltas
    }
}

fn handle_line(raw: &[u8], done: &mut bool, deltas: &mut Vec<String>) {
    let Ok(line) = std::str::from_utf8(raw) else {
        return; // a broken line is not worth failing the reply over
    };
    let line = line.trim_end_matches('\r');
    if line.is_empty() || !line.starts_with("data:") {
        return;
    }
    let payload = line["data:".len()..].trim();
    if payload.is_empty() {
        return;
    }
    if payload == "[DONE]" {
        *done = true;
        return;
    }
    let Ok(value) = serde_json::from_str::<Value>(payload) else {
        return; // non-JSON keep-alive / diagnostic line
    };
    if let Some(delta) = delta_of(&value) {
        if !delta.is_empty() {
            deltas.push(delta);
        }
    }
}

/// Pull `choices[0].delta.content`, falling back to the legacy `choices[0].text`.
fn delta_of(value: &Value) -> Option<String> {
    let choice = value.get("choices")?.get(0)?;
    if let Some(content) = choice
        .get("delta")
        .and_then(|delta| delta.get("content"))
        .and_then(Value::as_str)
    {
        return Some(content.to_string());
    }
    choice
        .get("text")
        .and_then(Value::as_str)
        .map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sse(text: &str) -> String {
        format!("data: {text}\n\n")
    }

    fn content(delta: &str) -> String {
        sse(&json!({"choices": [{"delta": {"content": delta}}]}).to_string())
    }

    fn collect(parser: &mut SseParser, chunks: &[&[u8]]) -> Vec<String> {
        let mut deltas = Vec::new();
        for chunk in chunks {
            deltas.extend(parser.push(chunk));
        }
        deltas.extend(parser.finish());
        deltas
    }

    #[test]
    fn single_chunk_yields_one_delta_and_stops_at_done() {
        let mut parser = SseParser::new();
        let body = format!("{}{}", content("hello"), sse("[DONE]"));
        let deltas = collect(&mut parser, &[body.as_bytes()]);
        assert_eq!(deltas, vec!["hello".to_string()]);
        assert!(parser.is_done());
    }

    #[test]
    fn a_line_split_across_chunks_is_reassembled() {
        let mut parser = SseParser::new();
        let body = format!("{}{}", content("split me"), sse("[DONE]"));
        let bytes = body.as_bytes();
        let (left, right) = bytes.split_at(bytes.len() / 2);
        let deltas = collect(&mut parser, &[left, right]);
        assert_eq!(deltas, vec!["split me".to_string()]);
        assert!(parser.is_done());
    }

    #[test]
    fn a_utf8_character_split_across_chunks_survives() {
        let mut parser = SseParser::new();
        let body = content("识别");
        let bytes = body.as_bytes();
        // Split inside the three-byte UTF-8 sequence of "识".
        let split = body.find('识').unwrap() + 1;
        assert!(split < bytes.len());
        let deltas = collect(&mut parser, &[&bytes[..split], &bytes[split..]]);
        assert_eq!(deltas, vec!["识别".to_string()]);
    }

    #[test]
    fn multiple_deltas_keep_their_order() {
        let mut parser = SseParser::new();
        let body = format!(
            "{}{}{}{}",
            content("fn "),
            content("main"),
            content("() {}"),
            sse("[DONE]")
        );
        let deltas = collect(&mut parser, &[body.as_bytes()]);
        assert_eq!(deltas, vec!["fn ", "main", "() {}"]);
    }

    #[test]
    fn nothing_after_done_is_parsed() {
        let mut parser = SseParser::new();
        let body = format!(
            "{}{}{}",
            content("first"),
            sse("[DONE]"),
            content("must not appear")
        );
        let deltas = collect(&mut parser, &[body.as_bytes()]);
        assert_eq!(deltas, vec!["first".to_string()]);
        assert!(parser.is_done());
    }

    #[test]
    fn malformed_json_is_skipped_without_losing_later_deltas() {
        let mut parser = SseParser::new();
        let body = format!(
            "{}data: {{not json}}\n\n{}",
            content("before"),
            content("after")
        );
        let deltas = collect(&mut parser, &[body.as_bytes()]);
        assert_eq!(deltas, vec!["before", "after"]);
    }

    #[test]
    fn empty_response_produces_no_deltas_and_is_not_done() {
        let mut parser = SseParser::new();
        let deltas = collect(&mut parser, &[b"", b"\n\n", b": keep-alive\n\n"]);
        assert!(deltas.is_empty());
        assert!(!parser.is_done());
    }

    #[test]
    fn trailing_line_without_newline_is_flushed() {
        let mut parser = SseParser::new();
        let body = content("tail");
        let deltas = collect(&mut parser, &[body.trim_end().as_bytes()]);
        assert_eq!(deltas, vec!["tail".to_string()]);
        assert!(!parser.is_done());
    }
}
