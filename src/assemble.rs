//! Pair recognised text with geometric evidence.
//!
//! Two paths, chosen by what the engine gives us:
//! * **boxes** (Tesseract): cluster words into visual rows, rebuild inter-column
//!   gaps from x, take indentation from the row's first x.
//! * **no boxes** (GLM-OCR and friends): walk the text and the detected bands in
//!   parallel, take indentation from each band's left edge.
//!
//! The second path is why the core does not need a platform OCR: geometry comes
//! from the pixels, so a bbox-less VLM is enough.

use crate::engine::RawLine;
use crate::geom::{fit_advance, Geometry};
use crate::Line;

#[derive(Debug, Clone)]
pub struct Opts {
    /// merge rows that look like terminal soft-wraps into one logical line
    pub join_wrap: bool,
    /// detect a line-number gutter and trust the model's own spacing instead
    pub strip_gutter: bool,
    /// disable geometry entirely (debug: what the engine actually said)
    pub no_geometry: bool,
}

impl Default for Opts {
    fn default() -> Self {
        Self {
            // off by default: a false merge destroys line structure, a missed merge
            // is a one-second fix for the user. Pixel-only soft-wrap detection is
            // not reliable (a long command line looks exactly like a wrapped one).
            join_wrap: false,
            strip_gutter: true,
            no_geometry: false,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct AssembleStats {
    pub mode: &'static str,
    pub bands: usize,
    pub text_lines: usize,
    pub advance: f32,
    pub gutter: bool,
    pub joined_wraps: usize,
}

pub fn assemble(lines: &[RawLine], geom: &Geometry, opts: &Opts) -> (Vec<Line>, AssembleStats) {
    let mut texts: Vec<&RawLine> = lines.iter().collect();
    // a capture never meaningfully begins or ends with a blank row; models often
    // emit a leading newline, which would otherwise show up as an empty first line
    let first = texts
        .iter()
        .position(|l| !l.text.trim().is_empty())
        .unwrap_or(texts.len());
    texts.drain(..first);
    while texts
        .last()
        .map(|l| l.text.trim().is_empty())
        .unwrap_or(false)
    {
        texts.pop();
    }

    let has_boxes = !texts.is_empty() && texts.iter().all(|l| l.bbox.is_some());
    if opts.no_geometry || geom.bands.is_empty() {
        let out = texts
            .iter()
            .map(|l| Line {
                text: l.text.trim_end().to_string(),
                indent: 0,
                conf: l.conf,
                x0: l.bbox.map(|b| b.x0),
            })
            .collect();
        return (
            out,
            AssembleStats {
                mode: "none",
                bands: geom.bands.len(),
                text_lines: texts.len(),
                advance: 0.0,
                gutter: false,
                joined_wraps: 0,
            },
        );
    }

    if has_boxes {
        assemble_with_boxes(&texts, geom, opts)
    } else {
        assemble_with_bands(&texts, geom, opts)
    }
}


/// Cell width. Ink-width per character is stable for monospace text (side
/// bearings cancel over a long line), while a lattice fit on x0 offsets is
/// fragile: the left side bearing differs per first glyph, which is the same
/// order of magnitude as the lattice step. So: trust the ratio when we have
/// enough long lines, and only fall back to the lattice when we do not.
fn estimate_advance(ratios: &[f32], offsets: &[f32], typo: f32) -> f32 {
    if ratios.len() >= 3 {
        // the median ink-width-per-character is already a robust measurement;
        // clamping it against the typography guess only adds bias
        median(ratios).unwrap_or(typo).clamp(4.0, 60.0)
    } else {
        fit_advance(offsets, typo)
    }
}

fn assemble_with_boxes(
    texts: &[&RawLine],
    geom: &Geometry,
    opts: &Opts,
) -> (Vec<Line>, AssembleStats) {
    // 1. group fragments into visual rows
    let lh = if geom.line_height > 0.0 {
        geom.line_height
    } else {
        12.0
    };
    let mut rows: Vec<Vec<&RawLine>> = Vec::new();
    for l in texts {
        let b = l.bbox.unwrap();
        match rows.last_mut() {
            None => rows.push(vec![l]),
            Some(cur) => {
                let cy: f32 =
                    cur.iter().map(|x| x.bbox.unwrap().center_y()).sum::<f32>() / cur.len() as f32;
                if (b.center_y() - cy).abs() <= 0.55 * lh {
                    cur.push(l);
                } else {
                    rows.push(vec![l]);
                }
            }
        }
        let _ = b;
    }

    // 2. advance: ink-width prior refined by lattice, then rows -> text
    let ratios: Vec<f32> = texts
        .iter()
        .filter(|l| l.text.trim().len() >= 10)
        .map(|l| l.bbox.unwrap().width() / l.text.trim().chars().count() as f32)
        .filter(|r| *r > 0.5)
        .collect();
    let typo = geom.advance_prior();
    let offsets: Vec<f32> = texts
        .iter()
        .map(|l| l.bbox.unwrap().x0 - geom.left)
        .filter(|d| *d > 0.5)
        .collect();
    let adv = estimate_advance(&ratios, &offsets, typo);

    let mut out: Vec<Line> = Vec::new();
    for row in &rows {
        let mut sorted: Vec<&RawLine> = row.clone();
        sorted.sort_by(|a, b| {
            a.bbox
                .unwrap()
                .x0
                .partial_cmp(&b.bbox.unwrap().x0)
                .unwrap()
        });
        let mut s = String::new();
        let mut confs = Vec::new();
        for l in &sorted {
            let b = l.bbox.unwrap();
            let col = (((b.x0 - geom.left) / adv).round().max(0.0)) as usize;
            if s.chars().count() < col {
                let pad = col - s.chars().count();
                s.push_str(&" ".repeat(pad));
            } else if !s.is_empty() && !s.ends_with(' ') {
                s.push(' ');
            }
            s.push_str(l.text.trim());
            if let Some(c) = l.conf {
                confs.push(c);
            }
        }
        let first = sorted[0].bbox.unwrap();
        let indent = (((first.x0 - geom.left) / adv).round().max(0.0)) as u32;
        out.push(Line {
            text: strip_indent(&s, indent as usize),
            indent,
            conf: confs.first().copied(),
            x0: Some(first.x0),
        });
    }

    let joined = if opts.join_wrap {
        join_soft_wraps(&mut out, geom, adv)
    } else {
        0
    };
    (
        out,
        AssembleStats {
            mode: "boxes",
            bands: geom.bands.len(),
            text_lines: texts.len(),
            advance: adv,
            gutter: false,
            joined_wraps: joined,
        },
    )
}

fn assemble_with_bands(
    texts: &[&RawLine],
    geom: &Geometry,
    opts: &Opts,
) -> (Vec<Line>, AssembleStats) {
    let gutter = opts.strip_gutter && looks_like_gutter(texts);

    let nonblank: Vec<usize> = texts
        .iter()
        .enumerate()
        .filter(|(_, l)| !l.text.trim().is_empty())
        .map(|(i, _)| i)
        .collect();
    let typo = geom.advance_prior();
    let offsets: Vec<f32> = geom
        .bands
        .iter()
        .map(|b| b.x0 - geom.left)
        .filter(|d| *d > 0.5)
        .collect();

    // Pass 1: provisional advance (identity pairing when the counts agree).
    let mut adv = if nonblank.len() == geom.bands.len() && !gutter {
        let r = ratios_for(&nonblank, texts, geom, &|i| i);
        estimate_advance(&r, &offsets, typo)
    } else {
        estimate_advance(&[], &offsets, typo)
    };

    // Pass 2: align. Equal counts are taken as identity; anything else goes through
    // the width-aware alignment so a single split row cannot shift the rest.
    let map: Option<Vec<(usize, usize)>> = if gutter {
        None
    } else if nonblank.len() == geom.bands.len() {
        Some(nonblank.iter().enumerate().map(|(i, &t)| (t, i)).collect())
    } else {
        align_by_width(texts, &nonblank, &geom.bands, adv)
            .or_else(|| pair_indices(&nonblank, geom.bands.len()))
    };

    // Pass 3: re-estimate the advance from the chosen alignment.
    if let Some(m) = map.as_ref() {
        if !gutter {
            let r = ratios_for(&nonblank, texts, geom, &|i| {
                m.iter().find(|(t, _)| *t == i).map(|(_, b)| *b).unwrap_or(i)
            });
            adv = estimate_advance(&r, &offsets, typo);
        }
    }

    let pairs: Vec<(&RawLine, f32)> = texts
        .iter()
        .map(|l| {
            let n = l.text.trim().chars().count();
            (*l, n as f32)
        })
        .collect();

    let mut out: Vec<Line> = Vec::new();
    for (ti, l) in pairs.iter().enumerate() {
        let text = l.0.text.trim_end().to_string();
        let (indent, x0) = if gutter {
            (leading_spaces(&text) as u32, None)
        } else {
            match map.as_ref().and_then(|m| m.iter().find(|(t, _)| *t == ti)) {
                Some((_, bi)) => {
                    let b = geom.bands[*bi];
                    let col = (((b.x0 - geom.left) / adv).round().max(0.0)) as u32;
                    (col, Some(b.x0))
                }
                None => (leading_spaces(&text) as u32, None),
            }
        };
        out.push(Line {
            text: text.trim_start().to_string(),
            indent,
            conf: l.0.conf,
            x0,
        });
    }

    let joined = if opts.join_wrap && !gutter {
        join_soft_wraps(&mut out, geom, adv)
    } else {
        0
    };
    (
        out,
        AssembleStats {
            mode: "bands",
            bands: geom.bands.len(),
            text_lines: texts.len(),
            advance: adv,
            gutter,
            joined_wraps: joined,
        },
    )
}

/// Map text-line index -> band index, ignoring blank text lines (they occupy no
/// row in the image). Exact when the counts match; proportional otherwise
/// (engines occasionally add or drop a row).
fn pair_indices(nonblank: &[usize], n_bands: usize) -> Option<Vec<(usize, usize)>> {
    let n = nonblank.len();
    if n == 0 || n_bands == 0 {
        return None;
    }
    if n == n_bands {
        return Some(nonblank.iter().enumerate().map(|(i, &t)| (t, i)).collect());
    }
    let skew = (n as f32 - n_bands as f32).abs() / n_bands as f32;
    if skew > 0.25 {
        return None;
    }
    Some(
        nonblank
            .iter()
            .enumerate()
            .map(|(i, &t)| {
                let b = ((i as f32) * (n_bands as f32 - 1.0) / ((n as f32 - 1.0).max(1.0)))
                    .round() as usize;
                (t, b.min(n_bands - 1))
            })
            .collect(),
    )
}



/// ink width per character for long lines, given an index mapping.
fn ratios_for(
    nonblank: &[usize],
    texts: &[&RawLine],
    geom: &Geometry,
    band_of: &dyn Fn(usize) -> usize,
) -> Vec<f32> {
    let mut out = Vec::new();
    for &ti in nonblank {
        let t = texts[ti].text.trim();
        let n = t.chars().count();
        if n < 10 {
            continue;
        }
        let bi = band_of(ti);
        if bi >= geom.bands.len() {
            continue;
        }
        let b = geom.bands[bi];
        let r = (b.x1 - b.x0) / n as f32;
        if r > 0.5 {
            out.push(r);
        }
    }
    out
}

/// Monotone alignment of text lines to bands when the counts differ.
///
/// The engine sometimes splits one visual row into two lines, or merges two rows
/// into one. Plain proportional mapping then shifts *everything after* the split,
/// which is how indentation silently drifts. Instead we align by evidence:
/// a line's expected width (char count × advance) should match the ink width of
/// the band(s) it covers. Three moves: 1↔1, 2 texts on 1 band, 1 text over 2 bands.
fn align_by_width(
    texts: &[&crate::engine::RawLine],
    nonblank: &[usize],
    bands: &[crate::geom::Band],
    adv: f32,
) -> Option<Vec<(usize, usize)>> {
    let m = nonblank.len();
    let n = bands.len();
    if m == 0 || n == 0 {
        return None;
    }
    if m == n {
        return Some(nonblank.iter().enumerate().map(|(i, &t)| (t, i)).collect());
    }
    const SKIP: f32 = 0.05; // tiny penalty so 1:1 wins when widths are ambiguous
    let exp = |i: usize| texts[nonblank[i]].text.trim().chars().count() as f32 * adv;
    let bw = |j: usize| (bands[j].x1 - bands[j].x0).max(1.0);
    let mut cost = vec![f32::MAX; (m + 1) * (n + 1)];
    let mut from = vec![(0u8, 0u8); (m + 1) * (n + 1)];
    cost[0] = 0.0;
    let idx = |i: usize, j: usize| i * (n + 1) + j;
    for i in 0..=m {
        for j in 0..=n {
            let c = cost[idx(i, j)];
            if c == f32::MAX {
                continue;
            }
            let mut relax = |ni: usize, nj: usize, add: f32, tag: (u8, u8)| {
                let k = idx(ni, nj);
                if c + add < cost[k] {
                    cost[k] = c + add;
                    from[k] = tag;
                }
            };
            if i < m && j < n {
                let e = (exp(i) - bw(j)).abs() / bw(j);
                relax(i + 1, j + 1, e.min(1.0), (1, 1));
            }
            if i + 1 < m && j < n {
                // model split one row into two lines
                let e = ((exp(i) + exp(i + 1)) - bw(j)).abs() / bw(j);
                relax(i + 2, j + 1, e.min(1.0) + SKIP, (2, 1));
            }
            if i < m && j + 1 < n {
                // model merged two rows into one line
                let e = (exp(i) - (bw(j) + bw(j + 1))).abs() / (bw(j) + bw(j + 1));
                relax(i + 1, j + 2, e.min(1.0) + SKIP, (1, 2));
            }
        }
    }
    let best_end = (0..=n).map(|j| cost[idx(m, j)]).fold(f32::MAX, f32::min);
    if best_end == f32::MAX || best_end > m as f32 * 0.6 {
        return None; // alignment is not better than guessing
    }
    // walk back
    let mut pairs: Vec<(usize, usize)> = Vec::new();
    let mut i = m;
    let mut j = (0..=n).find(|&j| cost[idx(m, j)] == best_end).unwrap_or(n);
    while i > 0 && j > 0 {
        let (di, dj) = from[idx(i, j)];
        if di == 0 {
            break;
        }
        match (di, dj) {
            (1, 1) => pairs.push((nonblank[i - 1], j - 1)),
            (2, 1) => {
                pairs.push((nonblank[i - 1], j - 1));
                pairs.push((nonblank[i - 2], j - 1));
            }
            (1, 2) => pairs.push((nonblank[i - 1], j - 2)),
            _ => break,
        }
        i -= di as usize;
        j -= dj as usize;
    }
    pairs.reverse();
    Some(pairs)
}

/// Line-number gutters make x0 identical on every row, so geometry stops telling
/// us anything about indentation. Detect them and fall back to the model's own
/// spacing (see docs §4.6, the single failing case).
fn looks_like_gutter(texts: &[&RawLine]) -> bool {
    let mut hits = 0;
    let mut total = 0;
    for l in texts {
        let t = l.text.trim_start();
        if t.is_empty() {
            continue;
        }
        total += 1;
        let digits = t.chars().take_while(|c| c.is_ascii_digit()).count();
        if digits > 0 && digits <= 5 {
            let rest: String = t.chars().skip(digits).collect();
            if rest.starts_with(' ') || rest.starts_with('│') || rest.starts_with('|') {
                hits += 1;
            }
        }
    }
    total >= 3 && hits as f32 / total as f32 >= 0.4
}

fn join_soft_wraps(out: &mut Vec<Line>, geom: &Geometry, adv: f32) -> usize {
    if out.len() < 2 || geom.right <= 0.0 {
        return 0;
    }
    let edge = geom.right - adv * 1.5;
    let mut joined = 0;
    let mut i = 0;
    while i + 1 < out.len() {
        let cur_edge = out[i].x0.map(|_| ()).is_some();
        let _ = cur_edge;
        // a wrap: current row reaches the right margin, next row starts at the left
        let reaches_right = out[i]
            .x0
            .map(|x| x + adv * out[i].text.chars().count() as f32 >= edge)
            .unwrap_or(false);
        let next_left = out[i + 1].indent == 0;
        if reaches_right && next_left && !out[i + 1].text.trim().is_empty() {
            let tail = out[i + 1].text.clone();
            out[i].text.push_str(&tail);
            out.remove(i + 1);
            joined += 1;
        } else {
            i += 1;
        }
    }
    joined
}

fn strip_indent(s: &str, n: usize) -> String {
    let mut chars = s.chars();
    for _ in 0..n {
        if chars.clone().next() == Some(' ') {
            chars.next();
        } else {
            break;
        }
    }
    chars.as_str().trim_end().to_string()
}

fn leading_spaces(s: &str) -> usize {
    s.chars().take_while(|c| *c == ' ').count()
}

fn median(v: &[f32]) -> Option<f32> {
    if v.is_empty() {
        return None;
    }
    let mut s = v.to_vec();
    s.sort_by(|a, b| a.partial_cmp(b).unwrap());
    Some(s[s.len() / 2])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geom::Band;

    fn geom_with(bands: &[(f32, f32, f32, f32)]) -> Geometry {
        Geometry {
            width: 1200,
            height: 900,
            bands: bands
                .iter()
                .map(|&(y0, y1, x0, x1)| Band { y0, y1, x0, x1 })
                .collect(),
            left: bands.iter().map(|b| b.2).fold(f32::MAX, f32::min),
            right: bands.iter().map(|b| b.3).fold(0.0, f32::max),
            // ink height 26.7 -> typography prior 16.0 px per cell
            line_height: 26.7,
        }
    }

    #[test]
    fn bands_give_indentation_without_any_bbox() {
        // three 38px rows; second starts 48px right of the first = 3 cells of 16px
        let geom = geom_with(&[
            (14.0, 52.0, 14.0, 600.0),
            (52.0, 90.0, 62.0, 500.0),
            (90.0, 128.0, 14.0, 300.0),
        ]);
        let raw = vec![
            RawLine::plain("def f():"),
            RawLine::plain("return 1"),
            RawLine::plain("x = 2"),
        ];
        let (out, st) = assemble(&raw, &geom, &Opts::default());
        assert_eq!(st.mode, "bands");
        assert_eq!(out[0].indent, 0);
        assert_eq!(out[1].indent, 3);
        assert_eq!(out[2].indent, 0);
        assert_eq!(out[1].text, "return 1");
    }

    #[test]
    fn gutter_falls_back_to_model_spacing() {
        let geom = geom_with(&[
            (14.0, 52.0, 14.0, 600.0),
            (52.0, 90.0, 14.0, 600.0),
            (90.0, 128.0, 14.0, 600.0),
        ]);
        let raw = vec![
            RawLine::plain("  1 use std::io;"),
            RawLine::plain("  2 fn main() {"),
            RawLine::plain("  3     let x = 1;"),
        ];
        let (out, st) = assemble(&raw, &geom, &Opts::default());
        assert!(st.gutter, "gutter should be detected");
        // in gutter mode the model's own leading spacing is preserved verbatim,
        // including the code indentation *inside* the row
        assert_eq!(out[0].rendered(), "  1 use std::io;");
        assert_eq!(out[2].rendered(), "  3     let x = 1;");
    }

    #[test]
    fn boxes_rebuild_column_gaps() {
        let geom = geom_with(&[(14.0, 52.0, 10.0, 1100.0)]);
        let raw = vec![
            RawLine::with_box("NAME", crate::geom::Rect::new(10.0, 14.0, 60.0, 52.0), None),
            RawLine::with_box("STATUS", crate::geom::Rect::new(330.0, 14.0, 420.0, 52.0), None),
        ];
        let (out, st) = assemble(&raw, &geom, &Opts::default());
        assert_eq!(st.mode, "boxes");
        assert_eq!(out.len(), 1);
        assert!(out[0].text.starts_with("NAME"));
        assert!(out[0].text.contains("STATUS"));
        // 320px gap at ~16px per cell -> a gap of roughly 20 cells; the exact
        // value depends on the advance estimate, the *behaviour* is the column gap
        let gap = out[0].text.len() - "NAME".len() - "STATUS".len();
        assert!((12..=24).contains(&gap), "gap was {gap}, text {:?}", out[0].text);
    }


    #[test]
    fn width_alignment_absorbs_a_split_row() {
        // 4 text lines, 3 bands: the middle text pair was one visual row
        let geom = geom_with(&[
            (14.0, 52.0, 10.0, 110.0),
            (52.0, 90.0, 10.0, 310.0),
            (90.0, 128.0, 10.0, 210.0),
        ]);
        let raw = vec![
            RawLine::plain("aaaaaaaaaa"),             // 10 chars -> 100px
            RawLine::plain("bbbbbbbbbbbbbbb"),        // 15 chars -> 150px
            RawLine::plain("ccccccccccccccc"),        // 15 chars -> 150px
            RawLine::plain("dddddddddddddddddddd"),   // 20 chars -> 200px
        ];
        let (out, st) = assemble(&raw, &geom, &Opts::default());
        assert_eq!(st.bands, 3);
        let texts: Vec<&str> = out.iter().map(|l| l.text.as_str()).collect();
        assert_eq!(texts.len(), 4);
        // all four lines survive; the split pair shares the middle band
        assert!(texts[0].starts_with("aaaa"));
        assert!(texts[3].starts_with("dddd"));
    }

    #[test]
    fn proportional_pairing_when_counts_differ() {
        let idx: Vec<usize> = (0..10).collect();
        assert!(pair_indices(&idx, 10)
            .unwrap()
            .iter()
            .all(|(a, b)| a == b));
        assert!(pair_indices(&idx, 9).is_some());
        assert!(pair_indices(&idx, 3).is_none(), "too much skew -> give up");
        // blank lines occupy no band: 5 non-blank text lines, 5 bands
        let sparse = vec![0, 2, 4, 6, 8];
        assert_eq!(
            pair_indices(&sparse, 5).unwrap(),
            vec![(0, 0), (2, 1), (4, 2), (6, 3), (8, 4)]
        );
    }
}
