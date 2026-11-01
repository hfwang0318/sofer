//! sofer core: turn code / terminal screenshots into text that survives a paste.
//!
//! The pipeline is deliberately engine-agnostic:
//!
//! ```text
//! image ─► engine (GLM-OCR / Tesseract / mock) ─► text lines
//!       └► geometry (pure CV, no OCR) ───────────► bands, x0, advance
//!                                                   │
//!              assemble (pair, gutter, soft-wrap) ──┘
//!                     │
//!               repair (dashes, quotes, full-width)
//!                     │
//!               render (text / markdown / json)
//! ```
//!
//! Geometry never depends on a platform API, which is what lets the same core run
//! on macOS and Windows with identical output.

pub mod assemble;
pub mod boxdraw;
pub mod cli_input;
pub mod engine;
pub mod geom;
pub mod lang;
pub mod pipeline;
pub mod pool;
pub mod render;
pub mod repair;
pub mod tile;

pub use engine::{Engine, EngineOutput, ImageFormat, ImageInput, RawLine};
pub use geom::{Band, Geometry, Rect};

/// A finished line of text: content, indentation, and where it came from.
#[derive(Debug, Clone, serde::Serialize)]
pub struct Line {
    pub text: String,
    pub indent: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub conf: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub x0: Option<f32>,
}

impl Line {
    pub fn rendered(&self) -> String {
        format!("{}{}", " ".repeat(self.indent as usize), self.text)
    }
}

/// Fully assembled document.
#[derive(Debug, Clone, serde::Serialize)]
pub struct Document {
    pub engine: String,
    pub source: String,
    pub ms: u64,
    pub width: u32,
    pub height: u32,
    pub lines: Vec<Line>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
}

impl Document {
    pub fn text(&self) -> String {
        let mut out = String::new();
        for (i, l) in self.lines.iter().enumerate() {
            if i > 0 {
                out.push('\n');
            }
            out.push_str(&l.rendered());
        }
        out
    }
}
