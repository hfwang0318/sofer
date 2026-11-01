//! Text repair: undo the substitutions OCR makes that a compiler will not forgive.
//!
//! Kept deliberately conservative. Anything that needs world knowledge (fixing
//! `l` vs `1` inside identifiers) is opt-in and language-gated, because a wrong
//! "fix" is worse than a visible mistake.

/// Characters that OCR swaps in for their ASCII twins, mapped back.
const DASHES: &[(char, &str)] = &[
    ('\u{2014}', "-"), // em dash
    ('\u{2013}', "-"), // en dash
    ('\u{2212}', "-"), // minus sign
    ('\u{2015}', "-"), // horizontal bar
    ('\u{2010}', "-"), // hyphen
    ('\u{2011}', "-"), // non-breaking hyphen
];

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
    let chars: Vec<char> = input.chars().collect();
    let mut out = String::with_capacity(input.len());
    for (i, &ch) in chars.iter().enumerate() {
        let prev = i.checked_sub(1).map(|j| chars[j]);
        let next = chars.get(i + 1).copied();
        let mut replaced = false;
        for (from, to) in DASHES {
            if ch == *from {
                out.push_str(to);
                replaced = true;
                break;
            }
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

pub fn normalize(lines: &mut [crate::Line]) {
    for l in lines.iter_mut() {
        let t = normalize_line(&l.text);
        l.text = t;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn em_dash_becomes_hyphen() {
        assert_eq!(normalize_line("git diff \u{2014}stat"), "git diff -stat");
        assert_eq!(normalize_line("a\u{2013}b"), "a-b");
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
}
