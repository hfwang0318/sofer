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
    C,
    Cpp,
    Java,
    Kotlin,
    Swift,
    CSharp,
    Ruby,
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
            Lang::C => "c",
            Lang::Cpp => "cpp",
            Lang::Java => "java",
            Lang::Kotlin => "kotlin",
            Lang::Swift => "swift",
            Lang::CSharp => "csharp",
            Lang::Ruby => "ruby",
            Lang::Sql => "sql",
            Lang::Diff => "diff",
            Lang::Text => "text",
        }
    }
}

/// Keywords are already lower-case: `guess` lower-cases the whole capture once,
/// so re-lower-casing every key on every comparison is wasted work.
const FN_KEYWORDS: &[(Lang, &[&str])] = &[
    (
        Lang::Rust,
        &[
            "fn ",
            "let mut ",
            "impl ",
            "pub fn",
            "use std",
            "-> result",
            "#[derive",
            "match ",
        ],
    ),
    (
        Lang::Python,
        &[
            "def ", "elif ", "self.", "import ", "from ", "__init__", "f\"", "print(",
        ],
    ),
    (
        Lang::TypeScript,
        &[
            "interface ",
            "export const",
            ": string",
            ": number",
            "async function",
            "await ",
        ],
    ),
    (
        Lang::JavaScript,
        &["const ", "=> {", "function(", "console.log", "require("],
    ),
    (
        Lang::Shell,
        &[
            "$ ", "sudo ", "apt ", "brew ", "export ", "#!/bin/", "| grep", "chmod ",
        ],
    ),
    (Lang::Json, &["\"name\":", "\"version\":", "},\n{", "[\n{"]),
    (Lang::Go, &["func ", "package ", ":=", "fmt.", "goroutine"]),
    (
        Lang::C,
        &["#include <", "#include \"", "printf(", "int main("],
    ),
    (
        Lang::Cpp,
        &["#include <iostream>", "std::", "nullptr", "template<"],
    ),
    (
        Lang::Java,
        &["public class ", "system.out.", "private ", "@override"],
    ),
    (
        Lang::Kotlin,
        &["fun main", "val ", "var ", "println(", "data class "],
    ),
    (
        Lang::Swift,
        &["func ", "guard let", "import foundation", "struct "],
    ),
    (
        Lang::CSharp,
        &[
            "namespace ",
            "public void",
            "console.writeline",
            "using system",
        ],
    ),
    (Lang::Ruby, &["def ", "require ", "puts ", "end\n"]),
    (
        Lang::Sql,
        &["select ", "from ", "where ", "insert into", "group by"],
    ),
];

pub fn guess(text: &str) -> Lang {
    let lower = text.to_lowercase();
    if text
        .lines()
        .any(|l| l.starts_with("diff --git ") || l.starts_with("@@ "))
    {
        return Lang::Diff;
    }
    // A shebang names its interpreter outright; no keyword counting needed.
    if let Some(first) = text.lines().next() {
        if let Some(rest) = first.trim_start().strip_prefix("#!") {
            let rest = rest.to_lowercase();
            if rest.contains("python") {
                return Lang::Python;
            }
            if rest.contains("node") {
                return Lang::JavaScript;
            }
            if rest.contains("ruby") {
                return Lang::Ruby;
            }
            if ["sh", "bash", "zsh", "fish"]
                .iter()
                .any(|s| rest.contains(s))
            {
                return Lang::Shell;
            }
        }
    }
    // A captured *terminal session* is shell output even if it happens to contain
    // python keywords (tracebacks, `def` in a stack frame, ...).
    if text.lines().take(3).any(|l| {
        l.trim_start().starts_with("$ ") || l.contains(" $ ") || l.trim_start().starts_with("~/")
    }) {
        return Lang::Shell;
    }
    let mut best = (Lang::Text, 0usize);
    for (lang, keys) in FN_KEYWORDS {
        let mut score = 0;
        for k in *keys {
            score += lower.matches(k).count();
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
        assert_eq!(
            guess("~/proj $ pytest -q\n    def test_x():\n"),
            Lang::Shell
        );
    }

    #[test]
    fn a_shebang_names_the_interpreter() {
        assert_eq!(guess("#!/usr/bin/env python3\nprint(1)"), Lang::Python);
        assert_eq!(guess("#!/bin/bash\nset -e"), Lang::Shell);
    }

    #[test]
    fn detects_c_and_cpp() {
        assert_eq!(guess("#include <stdio.h>\nint main(void) {"), Lang::C);
        assert_eq!(
            guess("#include <iostream>\nstd::string s = nullptr;"),
            Lang::Cpp
        );
    }

    #[test]
    fn falls_back_to_text() {
        assert_eq!(guess("hello world"), Lang::Text);
    }
}
