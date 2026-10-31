//! Output formats. The product's real interface is "paste this into an editor or
//! an LLM", so the markdown form is the default in spirit even if `text` is the
//! safe default for piping.

use crate::{Document, Line};

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum, serde::Serialize, serde::Deserialize,
)]
#[serde(rename_all = "lowercase")]
pub enum Format {
    Text,
    Markdown,
    Json,
}

pub fn render(doc: &Document, format: Format, fence_lang: Option<&str>) -> String {
    match format {
        Format::Text => doc.text(),
        Format::Markdown => {
            let lang = fence_lang.unwrap_or_else(|| doc.language.as_deref().unwrap_or("text"));
            format!("```{lang}\n{}\n```", doc.text())
        }
        Format::Json => serde_json::to_string_pretty(doc).unwrap_or_else(|e| e.to_string()),
    }
}

/// Indentation summary, used by `--stats` and the baseline harness.
pub fn indent_profile(lines: &[Line]) -> Vec<(usize, String)> {
    lines
        .iter()
        .enumerate()
        .map(|(i, l)| (i, l.rendered()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc() -> Document {
        Document {
            engine: "mock".into(),
            source: "x.png".into(),
            ms: 1,
            width: 10,
            height: 10,
            lines: vec![
                Line { text: "def f():".into(), indent: 0, conf: None, x0: None },
                Line { text: "return 1".into(), indent: 4, conf: None, x0: None },
            ],
            language: Some("python".into()),
        }
    }

    #[test]
    fn text_keeps_indentation() {
        assert_eq!(render(&doc(), Format::Text, None), "def f():\n    return 1");
    }

    #[test]
    fn markdown_fences_with_language() {
        let s = render(&doc(), Format::Markdown, None);
        assert!(s.starts_with("```python\n"));
        assert!(s.ends_with("\n```"));
    }

    #[test]
    fn json_round_trips() {
        let s = render(&doc(), Format::Json, None);
        let v: serde_json::Value = serde_json::from_str(&s).unwrap();
        assert_eq!(v["lines"][1]["indent"], 4);
    }
}
