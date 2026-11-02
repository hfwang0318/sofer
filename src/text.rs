//! Terminal-column arithmetic for OCR text.
//!
//! The geometry layer measures ink in *pixels*; the text layer counts *chars*.
//! Those two only agree for ASCII. A CJK ideograph occupies two monospace cells
//! but is one `char`, and a zero-width joiner occupies none. Every place that
//! compares an expected text width against a pixel width has to use the same
//! unit, or `advance` and the alignment cost drift on mixed-language captures.

/// Columns occupied by one character on a terminal.
///
/// Deliberately scoped to the ranges that matter for screenshots: CJK, Hangul,
/// full-width forms and emoji are 2; zero-width marks are 0; everything else is 1.
pub fn char_width(c: char) -> usize {
    let u = c as u32;
    if is_zero_width(u) {
        return 0;
    }
    if is_wide(u) {
        2
    } else {
        1
    }
}

fn is_zero_width(u: u32) -> bool {
    matches!(
        u,
        0x00AD | 0x200B..=0x200F | 0x202A..=0x202E | 0x2060..=0x2064 | 0xFEFF
    )
}

fn is_wide(u: u32) -> bool {
    matches!(
        u,
        // Hangul Jamo
        0x1100..=0x115F
        // CJK radicals, Kangxi, CJK symbols and punctuation, Hiragana/Katakana,
        // Bopomofo, Hangul compatibility, CJK strokes, enclosed CJK
        | 0x2E80..=0x303E
        // CJK unified ideographs + extensions A (through Yi and Hangul syllables)
        | 0x3041..=0xA4CF
        // Hangul syllables
        | 0xAC00..=0xD7A3
        // Private use / compatibility ideographs
        | 0xF900..=0xFAFF
        // Vertical and CJK compatibility forms
        | 0xFE10..=0xFE19
        | 0xFE30..=0xFE6F
        // Fullwidth ASCII variants and fullwidth signs
        | 0xFF00..=0xFF60
        | 0xFFE0..=0xFFE6
        // Wide emoji / pictographs
        | 0x1F300..=0x1F64F
        | 0x1F900..=0x1F9FF
        // CJK extension B and beyond
        | 0x20000..=0x3FFFD
    )
}

/// Columns occupied by a whole line.
pub fn display_width(s: &str) -> usize {
    s.chars().map(char_width).sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ascii_is_one_column_per_char() {
        assert_eq!(display_width("let x = 1;"), 10);
    }

    #[test]
    fn cjk_and_fullwidth_are_two_columns() {
        assert_eq!(display_width("启动服务"), 8);
        assert_eq!(display_width("（）"), 4);
        assert_eq!(display_width("，"), 2);
    }

    #[test]
    fn zero_width_characters_take_no_space() {
        assert_eq!(display_width("a\u{200b}b"), 2);
        assert_eq!(display_width("\u{feff}fn"), 2);
        assert_eq!(display_width("ab\u{200d}cd"), 4);
    }

    #[test]
    fn mixed_text_sums_per_character() {
        assert_eq!(display_width("fn 启动()"), 9);
    }
}
