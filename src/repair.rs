//! Text repair: undo the substitutions OCR makes that a compiler will not forgive.
//!
//! Kept deliberately conservative. Anything that needs world knowledge (fixing
//! `l` vs `1` inside identifiers) is opt-in and language-gated, because a wrong
//! "fix" is worse than a visible mistake.
//!
//! Two of the rules depend on what kind of document this is: an em dash in a
//! `git diff` is a lost `--`, while the same character in prose is a dash. So
//! the document is classified once, before any line is rewritten.

use crate::engine::RawLine;
use crate::Line;

/// What the capture mostly contains. Only used to pick repair rules.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Code,
    Diff,
    Terminal,
    Table,
    Prose,
}

impl Kind {
    /// Guess from the model's raw text. Deliberately checks the strongest
    /// markers first: a diff also contains code, and a terminal session also
    /// contains prose.
    pub fn of(texts: &[&str]) -> Self {
        let nonblank: Vec<&str> = texts
            .iter()
            .copied()
            .filter(|t| !t.trim().is_empty())
            .collect();
        if nonblank.is_empty() {
            return Kind::Prose;
        }
        let starts = |p: &str| nonblank.iter().any(|t| t.trim_start().starts_with(p));
        if starts("diff --git ") || starts("@@ ") || (starts("+++ ") && starts("--- ")) {
            return Kind::Diff;
        }
        // A prompt line is the one marker a code snippet cannot fake.
        if nonblank
            .iter()
            .any(|t| t.trim_start().starts_with("$ ") || t.contains(" ~/ "))
        {
            return Kind::Terminal;
        }
        let code = nonblank.iter().filter(|t| looks_like_code(t)).count();
        if code * 100 >= nonblank.len() * 60 {
            return Kind::Code;
        }
        if nonblank.len() >= 3 && looks_like_table(&nonblank) {
            return Kind::Table;
        }
        Kind::Prose
    }
}

fn looks_like_code(line: &str) -> bool {
    let t = line.trim_end();
    t.ends_with(';')
        || t.ends_with('{')
        || t.ends_with('}')
        || t.ends_with(':')
        || t.contains("=>")
        || t.contains(" = ")
        || t.starts_with("fn ")
        || t.starts_with("def ")
        || t.starts_with("let ")
        || t.starts_with("const ")
        || t.starts_with("import ")
        || t.starts_with("from ")
        || t.starts_with("#include")
        || t.starts_with("#[")
        || t.starts_with("use ")
        || t.starts_with("pub ")
}

/// Two columns separated by a run of spaces, on most rows.
fn looks_like_table(lines: &[&str]) -> bool {
    let hits = lines
        .iter()
        .filter(|t| {
            let mut gaps = 0;
            let mut run = 0;
            for c in t.chars() {
                if c == ' ' {
                    run += 1;
                } else {
                    if run >= 2 {
                        gaps += 1;
                    }
                    run = 0;
                }
            }
            if run >= 2 {
                gaps += 1;
            }
            gaps >= 2
        })
        .count();
    hits * 100 >= lines.len() * 60
}

/// Characters that OCR swaps in for their ASCII twins, mapped back.
/// The em dash has two answers: one hyphen in code, two in a diff where the
/// model lost `--`.
fn dash_for(kind: Kind, ch: char, prev: Option<char>, next: Option<char>) -> Option<&'static str> {
    match ch {
        // Only an em dash standing alone after whitespace can be a lost `--`.
        // `-—stat` already carries the first hyphen, and in `----——` the two
        // em dashes are two ordinary hyphens, not two pairs.
        '\u{2014}' | '\u{2015}'
            if kind == Kind::Diff
                && prev.is_none_or(|c| c.is_whitespace())
                && next.is_some_and(|c| c.is_alphanumeric()) =>
        {
            Some("--")
        }
        '\u{2014}' | '\u{2015}' | '\u{2013}' | '\u{2212}' | '\u{2010}' | '\u{2011}' => Some("-"),
        _ => None,
    }
}

/// Full-width punctuation is ambiguous: it is legitimate in Chinese prose
/// ("启动服务，监听") but is also what OCR produces when it mistakes an ASCII
/// parenthesis for its CJK twin ("main（host）"). Fold it only when it touches
/// ASCII, i.e. when it is almost certainly code.
fn touches_ascii(prev: Option<char>, next: Option<char>) -> bool {
    let ascii = |c: Option<char>| {
        c.map(|c| c.is_ascii() && !c.is_whitespace())
            .unwrap_or(false)
    };
    ascii(prev) || ascii(next)
}

pub fn normalize_line(input: &str) -> String {
    normalize_line_as(Kind::Code, input)
}

pub fn normalize_line_as(kind: Kind, input: &str) -> String {
    let chars: Vec<char> = input.chars().collect();
    let mut out = String::with_capacity(input.len());
    for (i, &ch) in chars.iter().enumerate() {
        // Look past zero-width marks: `a\u{200b}（b）` is still ASCII on both
        // sides of the parenthesis, and folding must not be skipped because of
        // an invisible character OCR inserted.
        let prev = (0..i).rev().map(|j| chars[j]).find(|c| !is_ignorable(*c));
        let next = chars[i + 1..].iter().copied().find(|c| !is_ignorable(*c));
        let mut replaced = false;
        if let Some(to) = dash_for(kind, ch, prev, next) {
            out.push_str(to);
            replaced = true;
        }
        if replaced {
            continue;
        }
        match ch {
            '\u{00a0}' => out.push(' '),               // NBSP
            '\u{200b}' | '\u{200c}' | '\u{200d}' => {} // zero-width
            '\u{feff}' => {}                           // BOM
            '\u{2018}' | '\u{2019}' | '\u{201a}' => out.push('\''),
            '\u{201c}' | '\u{201d}' | '\u{201e}' => out.push('"'),
            '\u{ff01}'..='\u{ff5e}' if touches_ascii(prev, next) => {
                // full-width ASCII -> half-width, but only inside code (see above)
                let c = char::from_u32(ch as u32 - 0xfee0).unwrap_or(ch);
                out.push(c);
            }
            '\u{3000}' => out.push(' '), // ideographic space
            _ => out.push(ch),
        }
    }
    out.trim_end().to_string()
}

fn is_ignorable(c: char) -> bool {
    matches!(
        c,
        '\u{200b}' | '\u{200c}' | '\u{200d}' | '\u{200e}' | '\u{200f}' | '\u{feff}'
    )
}

pub fn normalize(lines: &mut [Line]) {
    let kind = Kind::of(&lines.iter().map(|l| l.text.as_str()).collect::<Vec<_>>());
    for l in lines.iter_mut() {
        l.text = normalize_line_as(kind, &l.text);
    }
}

/// Normalise before assembly: the geometry layer measures pixels, so it must
/// not be handed zero-width marks or full-width twins that change the text
/// width per character while the ink stays the same.
pub fn normalize_raw(lines: &mut [RawLine]) {
    let kind = Kind::of(&lines.iter().map(|l| l.text.as_str()).collect::<Vec<_>>());
    for l in lines.iter_mut() {
        l.text = normalize_line_as(kind, &l.text);
    }
}

/// Remove a Markdown fence the model wrapped the whole reply in, returning its
/// language tag. A fence in the middle of the reply is content (a fenced block
/// inside a log), so only a reply that is *entirely* wrapped is stripped.
pub fn strip_fence(lines: &mut Vec<RawLine>) -> Option<String> {
    let first = lines.iter().position(|l| !l.text.trim().is_empty())?;
    let last = lines.iter().rposition(|l| !l.text.trim().is_empty())?;
    if first >= last {
        return None;
    }
    let open = lines[first].text.trim_start();
    let ticks = open.chars().take_while(|c| *c == '`').count();
    if ticks < 3 {
        return None;
    }
    let close = lines[last].text.trim();
    if close.chars().count() < ticks || !close.chars().all(|c| c == '`') {
        return None;
    }
    if lines[first + 1..last]
        .iter()
        .any(|l| l.text.trim_start().starts_with("```"))
    {
        return None;
    }
    let lang = open.trim_start_matches('`').trim().to_string();
    lines.drain(..=first);
    while lines
        .last()
        .map(|l| l.text.trim().is_empty())
        .unwrap_or(false)
    {
        lines.pop();
    }
    lines.pop();
    while lines
        .last()
        .map(|l| l.text.trim().is_empty())
        .unwrap_or(false)
    {
        lines.pop();
    }
    Some(lang)
}

/// Collapse runs of three or more identical lines (whitespace-insensitive).
///
/// Used only when the reply has far more rows than the image has text bands:
/// two identical lines are normal log content, three in a row is the model
/// repeating itself.
pub fn dedup_repeated(lines: &mut Vec<RawLine>) -> usize {
    let squash = |s: &str| s.split_whitespace().collect::<String>();
    let mut out: Vec<RawLine> = Vec::with_capacity(lines.len());
    let mut removed = 0;
    let mut i = 0;
    while i < lines.len() {
        let key = squash(&lines[i].text);
        let mut j = i + 1;
        while j < lines.len() && !key.is_empty() && squash(&lines[j].text) == key {
            j += 1;
        }
        let run = j - i;
        if run >= 3 {
            out.push(lines[i].clone());
            removed += run - 1;
        } else {
            out.extend(lines[i..j].iter().cloned());
        }
        i = j;
    }
    *lines = out;
    removed
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plain(s: &str) -> RawLine {
        RawLine::plain(s.to_string())
    }

    #[test]
    fn em_dash_becomes_hyphen() {
        assert_eq!(normalize_line("git diff \u{2014}stat"), "git diff -stat");
        assert_eq!(normalize_line("a\u{2013}b"), "a-b");
    }

    #[test]
    fn em_dash_is_two_hyphens_in_a_diff() {
        let kind = Kind::of(&["diff --git a/x b/x", "@@ -1 +1 @@", "\u{2014}stat"]);
        assert_eq!(kind, Kind::Diff);
        assert_eq!(
            normalize_line_as(kind, "git diff \u{2014}stat"),
            "git diff --stat"
        );
        // a hyphen followed by a stray em dash is still one `--`, not three
        assert_eq!(
            normalize_line_as(kind, "git diff -\u{2014}stat"),
            "git diff --stat"
        );
        // two em dashes are two hyphens, not two pairs
        assert_eq!(
            normalize_line_as(kind, "git diff \u{2014}\u{2014}stat"),
            "git diff --stat"
        );
        // a trailing run of em dashes in a diff stat is decoration, not a flag
        assert_eq!(normalize_line_as(kind, "++++\u{2014}\u{2014}"), "++++--");
    }

    #[test]
    fn smart_quotes_are_ascii() {
        assert_eq!(normalize_line("\u{201c}x\u{201d}"), "\"x\"");
    }

    #[test]
    fn fullwidth_folds_only_in_code_context() {
        // OCR turned ASCII parens into CJK ones around code -> fold
        assert_eq!(normalize_line("main\u{ff08}host\u{ff09}"), "main(host)");
        // legitimate Chinese punctuation inside Chinese prose -> keep
        assert_eq!(
            normalize_line("# \u{542f}\u{52a8}\u{670d}\u{52a1}\u{ff0c}\u{76d1}\u{542c} 8080"),
            "# \u{542f}\u{52a8}\u{670d}\u{52a1}\u{ff0c}\u{76d1}\u{542c} 8080"
        );
    }

    #[test]
    fn a_zero_width_mark_does_not_hide_an_ascii_neighbour() {
        assert_eq!(
            normalize_line("main\u{200b}\u{ff08}host\u{ff09}"),
            "main(host)"
        );
    }

    #[test]
    fn keeps_box_drawing() {
        // box characters carry structure: do not touch them
        assert_eq!(
            normalize_line("\u{251c}\u{2500}\u{2500} cli"),
            "\u{251c}\u{2500}\u{2500} cli"
        );
    }

    #[test]
    fn strips_trailing_space_only() {
        assert_eq!(normalize_line("  x = 1   "), "  x = 1");
    }

    #[test]
    fn a_wrapped_reply_loses_its_fence_and_keeps_the_language() {
        let mut lines = vec![plain("```rust"), plain("fn main() {}"), plain("```")];
        assert_eq!(strip_fence(&mut lines).as_deref(), Some("rust"));
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].text, "fn main() {}");
    }

    #[test]
    fn a_fence_inside_the_reply_is_content() {
        let mut lines = vec![
            plain("```bash"),
            plain("$ echo hi"),
            plain("```"),
            plain("more output"),
        ];
        assert!(strip_fence(&mut lines).is_none());
        assert_eq!(lines.len(), 4, "nothing may be removed");
    }

    #[test]
    fn three_identical_lines_collapse_to_one() {
        let mut lines = vec![
            plain("build ok"),
            plain("build ok"),
            plain("build ok"),
            plain("done"),
        ];
        assert_eq!(dedup_repeated(&mut lines), 2);
        assert_eq!(lines.len(), 2);
    }

    #[test]
    fn two_identical_lines_are_real_content() {
        let mut lines = vec![plain("ok"), plain("ok"), plain("done")];
        assert_eq!(dedup_repeated(&mut lines), 0);
        assert_eq!(lines.len(), 3);
    }
}
