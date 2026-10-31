//! Guess what language a capture is, so the output can be fenced usefully.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    Rust,
    Python,
    JavaScript,
    TypeScript,
    Shell,
    Json,
    Go,
    Sql,
    Diff,
    Text,
}

impl Lang {
    pub fn fence(self) -> &'static str {
        match self {
            Lang::Rust => "rust",
            Lang::Python => "python",
            Lang::JavaScript => "javascript",
            Lang::TypeScript => "typescript",
            Lang::Shell => "bash",
            Lang::Json => "json",
            Lang::Go => "go",
            Lang::Sql => "sql",
            Lang::Diff => "diff",
            Lang::Text => "text",
        }
    }
}

const FN_KEYWORDS: &[(Lang, &[&str])] = &[
    (Lang::Rust, &["fn ", "let mut ", "impl ", "pub fn", "use std", "-> Result", "#[derive", "match "]),
    (Lang::Python, &["def ", "elif ", "self.", "import ", "from ", "__init__", "f\"", "print("]),
    (Lang::TypeScript, &["interface ", "export const", ": string", ": number", "async function", "await "]),
    (Lang::JavaScript, &["const ", "=> {", "function(", "console.log", "require("]),
    (Lang::Shell, &["$ ", "sudo ", "apt ", "brew ", "export ", "#!/bin/", "| grep", "chmod "]),
    (Lang::Json, &["\"key\"", "\"name\":", "{ \"", "},\n"]),
    (Lang::Go, &["func ", "package ", ":=", "fmt.", "goroutine"]),
    (Lang::Sql, &["SELECT ", "FROM ", "WHERE ", "INSERT INTO", "GROUP BY"]),
];

pub fn guess(text: &str) -> Lang {
    let lower = text.to_lowercase();
    if text.lines().any(|l| l.starts_with("diff --git ") || l.starts_with("@@ ")) {
        return Lang::Diff;
    }
    // A captured *terminal session* is shell output even if it happens to contain
    // python keywords (tracebacks, `def` in a stack frame, ...).
    if text
        .lines()
        .take(3)
        .any(|l| l.trim_start().starts_with("$ ") || l.contains(" $ ") || l.trim_start().starts_with("~/"))
    {
        return Lang::Shell;
    }
    let mut best = (Lang::Text, 0usize);
    for (lang, keys) in FN_KEYWORDS {
        let mut score = 0;
        for k in *keys {
            score += lower.matches(&k.to_lowercase()).count();
        }
        if score > best.1 {
            best = (*lang, score);
        }
    }
    if best.1 == 0 {
        Lang::Text
    } else {
        best.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_rust_and_python() {
        assert_eq!(guess("fn main() {\n    let mut x = 1;\n}"), Lang::Rust);
        assert_eq!(guess("def f(self):\n    return 1"), Lang::Python);
    }

    #[test]
    fn detects_diff() {
        assert_eq!(guess("diff --git a/x b/x\n@@ -1 +1 @@"), Lang::Diff);
    }

    #[test]
    fn terminal_session_wins_over_embedded_keywords() {
        assert_eq!(guess("~/proj $ pytest -q\n    def test_x():\n"), Lang::Shell);
    }

    #[test]
    fn falls_back_to_text() {
        assert_eq!(guess("hello world"), Lang::Text);
    }
}
