//! Reconstruction of box-drawing art from the pixels.
//!
//! The vision model reads the text of a tree or a table correctly but does not
//! re-emit the drawing glyphs: on `ascii-table` 185 of 441 non-space characters
//! were box glyphs and the model emitted none of them. The geometry layer
//! already has what is needed to put them back: an ink mask, one band per
//! visible row, and the band bbox each assembled line was matched to.
//!
//! Two passes:
//!
//! * **tree** — a branch stroke gets `├──` when its vertical rule continues
//!   into the next band and `└──` when it stops; ancestor rules become `│`.
//!   A leading line number is kept and the prefix is aligned after it.
//! * **table** — confirmed frames are rebuilt as Unicode rules. A missing
//!   horizontal rule row is inserted between the cell rows the engine did
//!   emit: each data band is matched to a line by bbox overlap, so the text and
//!   band counts do **not** have to agree (the common failure is the model
//!   dropping the whole `┌──┬──┐` row).
//!
//! It is on by default (`Config::box_art`; `--no-box-art` disables) and
//! idempotent: text that already carries Unicode box glyphs is left alone. It
//! still refuses to guess when the pixels are a photo (background noise > 1.8)
//! or when a 1× screenshot was upscaled for the model — `pipeline` skips the
//! pass there, because geometry works in original pixels where a code `|` is
//! as wide as a branch stroke. The table pass also declines when a data band
//! has no matching text line or the rows are not consecutive.
//!
//! A missing glyph is a cosmetic bug; a moved word is not.

use std::cmp::Ordering;

use image::GrayImage;

use crate::geom::{self, Band, Geometry};
use crate::Line;

/// What a run changed, also reported through `--stats`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BoxArtStats {
    pub tree_lines: usize,
    pub table_rows: usize,
    pub separators: usize,
}

/// `├── ` / `└── ` occupy four terminal cells.
const BRANCH_WIDTH: usize = 4;
/// A tree deeper than this is either a false positive or not worth guessing.
const MAX_TREE_COL: usize = 64;

#[derive(Debug, Clone, Copy)]
struct HSeg {
    /// first and last+1 ink row of the stroke
    y0: u32,
    y1: u32,
    /// first and last+1 ink column of the stroke
    x0: u32,
    x1: u32,
}

#[derive(Debug, Clone, Copy)]
struct VSeg {
    x0: u32,
    x1: u32,
    y0: u32,
    y1: u32,
}

#[derive(Debug, Clone, Copy)]
struct RawBranch {
    x0: f32,
    down: bool,
}

#[derive(Debug, Clone, Copy)]
struct Branch {
    x0: f32,
    col: usize,
    down: bool,
}

#[derive(Debug, Clone, Copy)]
struct Column {
    x: f32,
    y0: f32,
    y1: f32,
}

#[derive(Debug, Clone)]
struct Table {
    cols: Vec<f32>,
    rules_y: Vec<f32>,
    x0: f32,
    y0: f32,
    y1: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RuleKind {
    Top,
    Mid,
    Bottom,
}

#[derive(Debug, Clone, Copy, Default)]
struct RuleRebuild {
    rows: usize,
    seps: usize,
}

/// Rewrite decorative prefixes in place. See the module docs for the contract.
///
/// The table pass may *insert* the horizontal rule rows the engine omitted, so
/// the line list is taken by value-and-mutate rather than as a slice.
pub fn apply(img: &GrayImage, geom: &Geometry, advance: f32, lines: &mut Vec<Line>) -> BoxArtStats {
    let mut stats = BoxArtStats::default();
    if geom.bands.is_empty() || lines.is_empty() || advance <= 0.5 {
        return stats;
    }

    let (w, h) = img.dimensions();
    let mask = geom::ink_mask(img);
    // A photographed screen has textured background and soft glyph edges; the
    // stroke detector then reads `+`/`-` bars as tree branches (measured on
    // `git-diff@photo`: 0.997 -> 0.965). Screenshots, including JPEG
    // re-encodes, are far cleaner: 0.2 for PNG, 1.1 for JPEG, 2.1 for the photo.
    if background_noise(img, &mask) > 1.8 {
        return stats;
    }
    let hint = advance.max(1.0);
    let line_h = geom.line_height.max(1.0);
    // Long enough to be a rule rather than a glyph stroke. `min_v` is also what
    // keeps ordinary vertical stems (cap height, < a full row) out.
    let min_h = ((hint * 1.2).max(8.0)) as u32;
    let min_v = (line_h * 0.5).max(6.0) as u32;
    let hsegs = h_segments(&mask, w, h, min_h);
    let vsegs = v_segments(&mask, w, h, min_v);

    let raw = find_branches(geom, &hsegs, &vsegs, line_h);
    let max_chars = lines
        .iter()
        .map(|l| l.text.trim().chars().count())
        .max()
        .unwrap_or(0);
    let cell = estimate_cell(&vsegs, &raw, hint, line_h, max_chars);
    // The ink of `├`/`│` starts a few pixels right of the cell box (left side
    // bearing), and `geom.left` is set by an ordinary text glyph on the first
    // line. Anchor the decoration lattice on the branch strokes themselves.
    let origin = lattice_origin(geom, &raw, cell);
    let branches: Vec<Option<Branch>> = raw
        .iter()
        .map(|r| {
            r.map(|r| Branch {
                x0: r.x0,
                col: ((r.x0 - origin) / cell).round().max(0.0) as usize,
                down: r.down,
            })
        })
        .collect();
    let origin = refine_origin(geom, &branches, cell).unwrap_or(origin);

    // The tree pass rewrites an existing prefix. Match a branch band to the
    // text line whose bbox overlaps it: the row counts no longer have to agree
    // exactly, because a blank line still owns a band in an editor and a tree
    // may span only part of the screenshot.
    let nonblank: Vec<usize> = lines
        .iter()
        .enumerate()
        .filter(|(_, l)| !l.text.trim().is_empty())
        .map(|(i, _)| i)
        .collect();
    // Callers that built the lines by hand (tests, the CLI debug view) carry no
    // bbox. Fall back to the one-row-per-band order in that case.
    let positional = !lines.iter().any(|l| l.bbox.is_some()) && nonblank.len() == geom.bands.len();
    let mut used = vec![false; lines.len()];
    for (bi, band) in geom.bands.iter().enumerate() {
        let Some(br) = branches[bi] else { continue };
        let matched = if positional {
            nonblank.get(bi).copied()
        } else {
            lines.iter().enumerate().position(|(li, l)| {
                !used[li]
                    && l.bbox
                        .is_some_and(|b| span_overlap(b.y0, b.y1, band.y0, band.y1) > 0.0)
            })
        };
        let Some(li) = matched else { continue };
        used[li] = true;
        let origin_col = (((origin - geom.left) / cell).round().max(0.0)) as usize;
        let changed = rewrite_tree_line(
            &mut lines[li],
            &vsegs,
            br,
            cell,
            line_h,
            origin,
            *band,
            origin_col,
        );
        if changed {
            stats.tree_lines += 1;
        }
    }

    let cols = columns(&vsegs, line_h, (cell * 0.3).max(2.0));
    // The engine routinely drops the whole `┌──┬──┐` row and keeps the cell
    // text. Rebuild those rows from the detected strokes first; the older
    // conservative pass then only sees tables the model already framed.
    let rebuilt = rebuild_rule_rows(lines, geom, &hsegs, &cols, cell, line_h);
    stats.table_rows += rebuilt.rows;
    stats.separators += rebuilt.seps;
    if rebuilt.rows == 0 {
        let nonblank: Vec<usize> = lines
            .iter()
            .enumerate()
            .filter(|(_, l)| !l.text.trim().is_empty())
            .map(|(i, _)| i)
            .collect();
        let (rows, seps) = rewrite_tables(lines, &nonblank, geom, &hsegs, &cols, cell, line_h);
        stats.table_rows += rows;
        stats.separators += seps;
    }
    stats
}

/// Standard deviation of the background pixels around the modal gray level.
///
/// This separates a screenshot (near-zero, even after JPEG) from a photo of a
/// screen (textured paper/moiré). Only the background is measured, so the ink
/// pattern itself does not bias it.
fn background_noise(img: &GrayImage, mask: &[bool]) -> f32 {
    let mut hist = [0u64; 256];
    for (i, p) in img.pixels().enumerate() {
        if !mask[i] {
            hist[p.0[0] as usize] += 1;
        }
    }
    let mode = hist
        .iter()
        .enumerate()
        .max_by_key(|(_, &count)| count)
        .map(|(value, _)| value as i32)
        .unwrap_or(0);
    let mut n = 0u64;
    let mut sum = 0f64;
    let mut sum2 = 0f64;
    for (i, p) in img.pixels().enumerate() {
        if mask[i] {
            continue;
        }
        let v = p.0[0] as i32;
        if (v - mode).unsigned_abs() <= 6 {
            let v = v as f64;
            n += 1;
            sum += v;
            sum2 += v * v;
        }
    }
    if n < 100 {
        return 0.0;
    }
    let mean = sum / n as f64;
    ((sum2 / n as f64 - mean * mean).max(0.0)).sqrt() as f32
}

// ---------------------------------------------------------------------------
// Stroke detection
// ---------------------------------------------------------------------------

fn h_segments(mask: &[bool], w: u32, h: u32, min_len: u32) -> Vec<HSeg> {
    let mut out: Vec<HSeg> = Vec::new();
    for y in 0..h {
        let base = (y * w) as usize;
        let row = &mask[base..base + w as usize];
        let mut x = 0usize;
        while x < row.len() {
            if !row[x] {
                x += 1;
                continue;
            }
            let start = x;
            while x < row.len() && row[x] {
                x += 1;
            }
            let end = x;
            if end - start < min_len as usize {
                continue;
            }
            let mut merged = false;
            for s in out.iter_mut().rev() {
                // segments are pushed in row order, so once the previous row is
                // out of reach every older segment is too
                if s.y1 < y {
                    break;
                }
                if s.y1 == y && close_span(s.x0, s.x1, start as u32, end as u32) {
                    s.y1 = y + 1;
                    s.x0 = s.x0.min(start as u32);
                    s.x1 = s.x1.max(end as u32);
                    merged = true;
                    break;
                }
            }
            if !merged {
                out.push(HSeg {
                    y0: y,
                    y1: y + 1,
                    x0: start as u32,
                    x1: end as u32,
                });
            }
        }
    }
    out
}

fn v_segments(mask: &[bool], w: u32, h: u32, min_len: u32) -> Vec<VSeg> {
    let mut out: Vec<VSeg> = Vec::new();
    for x in 0..w {
        let mut y = 0u32;
        while y < h {
            if !mask[(y * w + x) as usize] {
                y += 1;
                continue;
            }
            let start = y;
            while y < h && mask[(y * w + x) as usize] {
                y += 1;
            }
            let end = y;
            if end - start < min_len {
                continue;
            }
            let mut merged = false;
            for s in out.iter_mut().rev() {
                if s.x1 < x {
                    break;
                }
                if s.x1 == x && close_span(s.y0, s.y1, start, end) {
                    s.x1 = x + 1;
                    s.y0 = s.y0.min(start);
                    s.y1 = s.y1.max(end);
                    merged = true;
                    break;
                }
            }
            if !merged {
                out.push(VSeg {
                    x0: x,
                    x1: x + 1,
                    y0: start,
                    y1: end,
                });
            }
        }
    }
    out
}

fn close_span(a0: u32, a1: u32, b0: u32, b1: u32) -> bool {
    (a0 as i64 - b0 as i64).abs() <= 2 && (a1 as i64 - b1 as i64).abs() <= 2
}

fn span_overlap(a0: f32, a1: f32, b0: f32, b1: f32) -> f32 {
    (a1.min(b1) - a0.max(b0)).max(0.0)
}

fn has_vertical(vs: &[VSeg], x: f32, y0: f32, y1: f32, min_len: f32, min_overlap: f32) -> bool {
    vs.iter().any(|v| {
        let x0 = v.x0 as f32;
        let x1 = v.x1 as f32 - 1.0;
        x >= x0 - 2.0
            && x <= x1 + 2.0
            && (v.y1 - v.y0) as f32 >= min_len
            && span_overlap(v.y0 as f32, v.y1 as f32, y0, y1) >= min_overlap
    })
}

// ---------------------------------------------------------------------------
// Tree reconstruction
// ---------------------------------------------------------------------------

fn find_branches(geom: &Geometry, hs: &[HSeg], vs: &[VSeg], line_h: f32) -> Vec<Option<RawBranch>> {
    // A tree branch is ~2.5 cells of horizontal rule; a table rule is much
    // wider. The line height is a stable proxy for the cell size here.
    let max_w = (line_h * 2.8).max(48.0);
    let min_branch_w = line_h * 0.9;
    let mut xs: Vec<Option<f32>> = vec![None; geom.bands.len()];
    for (bi, band) in geom.bands.iter().enumerate() {
        let bh = (band.y1 - band.y0).max(1.0);
        let mut best: Option<HSeg> = None;
        for h in hs {
            let width = (h.x1 - h.x0) as f32;
            if width < min_branch_w || width > max_w {
                continue;
            }
            if span_overlap(h.y0 as f32, h.y1 as f32, band.y0, band.y1) <= 0.0 {
                continue;
            }
            // A branch stroke runs through the middle of its text row. A rule
            // that merely touches the band's edge belongs to the next row: this
            // is what stopped the root label `src` from being given a `└──`.
            let rel = (((h.y0 as f32 + h.y1 as f32) / 2.0) - band.y0) / bh;
            if !(0.2..=0.85).contains(&rel) {
                continue;
            }
            let x0 = h.x0 as f32;
            if x0 < geom.left - 2.0 {
                continue;
            }
            // the junction needs a vertical stroke; half a band tall is the
            // difference between a `└` stub and an ordinary glyph stem
            if !has_vertical(vs, x0, band.y0, band.y1, line_h * 0.35, bh * 0.5) {
                continue;
            }
            // Table rules terminate in a vertical at both ends; tree branches
            // end in blank space before the label.
            if has_vertical(vs, h.x1 as f32, band.y0, band.y1, line_h * 0.5, bh * 0.25) {
                continue;
            }
            if best.map(|b| x0 < b.x0 as f32).unwrap_or(true) {
                best = Some(*h);
            }
        }
        if let Some(h) = best {
            xs[bi] = Some(h.x0 as f32);
        }
    }

    let mut out = vec![None; geom.bands.len()];
    for bi in 0..geom.bands.len() {
        if let Some(x0) = xs[bi] {
            out[bi] = Some(RawBranch {
                x0,
                down: continues_below(geom, &xs, vs, bi, x0),
            });
        }
    }
    out
}

/// Decide `├` vs `└` from the *next line's branch column*, not merely from ink.
///
/// Text stems can sit exactly under a previous branch column (in the real
/// `tree` screenshot, `└── lib.rs` puts an `l` stem under the `args.rs` branch),
/// so a raw "is there a vertical below?" test misreads every such corner as a
/// tee. The tree structure disambiguates:
///
/// * next branch at the same column -> a sibling below -> `├`;
/// * next branch to the right, with a full-height rule at our column -> a
///   descendant below -> `├`;
/// * otherwise (next branch to the left / no next line) -> `└`.
fn continues_below(geom: &Geometry, xs: &[Option<f32>], vs: &[VSeg], bi: usize, x: f32) -> bool {
    let Some(next_x) = xs.get(bi + 1).copied().flatten() else {
        return false;
    };
    if (next_x - x).abs() <= 3.0 {
        return true;
    }
    if x >= next_x - 3.0 {
        return false;
    }
    let next = geom.bands[bi + 1];
    let nh = (next.y1 - next.y0).max(1.0);
    has_vertical(vs, x, next.y0, next.y1, nh * 0.6, nh * 0.6)
}

/// Recover the character advance from the tree's own decoration lattice.
///
/// `assemble` estimates the advance from `ink width / character count`, which is
/// inflated when the engine omitted the box glyphs: the band still spans the
/// decoration columns but the text it is divided by is shorter. Branch columns
/// are exactly four cells apart per tree level, so the smallest gap between them
/// is `4 × advance` — independent of the text.
fn estimate_cell(
    vs: &[VSeg],
    raw: &[Option<RawBranch>],
    hint: f32,
    line_h: f32,
    max_chars: usize,
) -> f32 {
    let xs: Vec<f32> = raw.iter().flatten().map(|b| b.x0).collect();
    if xs.is_empty() {
        // A framed table has no branches, but its vertical rules sit exactly on
        // cell boundaries. Fit the advance to their offsets instead of trusting
        // `hint`, which `assemble` inflated by dividing a wide band by the few
        // characters the engine actually emitted.
        let rules: Vec<f32> = vs
            .iter()
            .filter(|v| (v.y1 - v.y0) as f32 >= line_h * 0.7)
            .map(|v| (v.x0 as f32 + v.x1 as f32) / 2.0)
            .collect();
        if rules.len() >= 2 {
            let left = rules.iter().cloned().fold(f32::MAX, f32::min);
            let offsets: Vec<f32> = rules.iter().map(|x| x - left).collect();
            let prior = (line_h * 0.6).max(4.0);
            let fitted = geom::fit_advance(&offsets, prior);
            // A divisor of the true advance fits the lattice just as well and
            // would double every slot count. The longest text row is an upper
            // bound on how many cells the frame can hold, so use it to reject
            // that fit and fall back to the engine/assemble estimate.
            let widest = offsets.iter().cloned().fold(0.0f32, f32::max);
            let slots = if fitted > 1.0 {
                widest / fitted
            } else {
                f32::MAX
            };
            let plausible = max_chars == 0 || slots <= max_chars as f32 * 1.8;
            if fitted > 4.0 && plausible {
                return fitted;
            }
        }
        return hint;
    }
    let mut anchors = xs.clone();
    let mut clusters = cluster(&xs, 3.0);
    if clusters.len() < 2 {
        // An indented tree may only show one branch column. Its ancestor rules
        // are vertical strokes to the left of that column, one level apart.
        let leftmost = xs.iter().cloned().fold(f32::MAX, f32::min);
        anchors.extend(
            vs.iter()
                .filter(|v| (v.y1 - v.y0) as f32 >= line_h * 0.7)
                .map(|v| (v.x0 as f32 + v.x1 as f32) / 2.0)
                .filter(|x| *x < leftmost - 2.0),
        );
        clusters = cluster(&anchors, 3.0);
    }
    if clusters.len() >= 2 {
        if let Some(min_gap) = clusters
            .windows(2)
            .map(|w| w[1] - w[0])
            .filter(|g| *g > 4.0)
            .reduce(f32::min)
        {
            let cell = min_gap / 4.0;
            if (4.0..=200.0).contains(&cell) {
                return cell;
            }
        }
    }
    hint
}

/// First estimate of the x where column 0's stroke starts, from the branch
/// columns and the advance (see the call site for why `geom.left` is not it).
fn lattice_origin(geom: &Geometry, raw: &[Option<RawBranch>], cell: f32) -> f32 {
    let mut v: Vec<f32> = raw
        .iter()
        .flatten()
        .map(|b| {
            let col = ((b.x0 - geom.left) / cell).round();
            b.x0 - col * cell
        })
        .collect();
    if v.is_empty() {
        return geom.left;
    }
    v.sort_by(|a, b| a.partial_cmp(b).unwrap_or(Ordering::Equal));
    v[v.len() / 2]
}

/// Re-anchor after the columns are known, so rounding inside `lattice_origin`
/// cannot bias every ancestor by a fraction of a pixel.
fn refine_origin(geom: &Geometry, branches: &[Option<Branch>], cell: f32) -> Option<f32> {
    let mut v: Vec<f32> = branches
        .iter()
        .flatten()
        .map(|b| b.x0 - b.col as f32 * cell)
        .collect();
    if v.is_empty() {
        return None;
    }
    v.sort_by(|a, b| a.partial_cmp(b).unwrap_or(Ordering::Equal));
    Some(v[v.len() / 2].max(geom.left - cell))
}

fn cluster(values: &[f32], tol: f32) -> Vec<f32> {
    let mut v = values.to_vec();
    v.sort_by(|a, b| a.partial_cmp(b).unwrap_or(Ordering::Equal));
    let mut groups: Vec<Vec<f32>> = Vec::new();
    for x in v {
        match groups.last_mut() {
            Some(g) if (x - g[0]).abs() <= tol => g.push(x),
            _ => groups.push(vec![x]),
        }
    }
    groups
        .iter()
        .map(|g| g.iter().sum::<f32>() / g.len() as f32)
        .collect()
}

#[allow(clippy::too_many_arguments)]
fn rewrite_tree_line(
    line: &mut Line,
    vs: &[VSeg],
    br: Branch,
    cell: f32,
    line_h: f32,
    origin: f32,
    band: Band,
    origin_col: usize,
) -> bool {
    if br.col > MAX_TREE_COL {
        return false;
    }
    // A line-number gutter is text the pass must preserve: the tree prefix
    // belongs *after* the number, not before it (otherwise a numbered tree gets
    // `├── 3 ├── cli`).
    let (gutter, rest) = split_line_number(&line.text);
    // The engine already produced a real branch here (not an ASCII pipe or a
    // bare label): leave its prefix alone. Re-deriving it from a slightly
    // different advance would only move the label.
    let trimmed = rest.trim_start();
    if trimmed.starts_with("├──") || trimmed.starts_with("└──") {
        return false;
    }
    // The branch must land on the recovered lattice; a half-cell residual means
    // the stroke is probably text, not decoration.
    let expected = origin + br.col as f32 * cell;
    if (br.x0 - expected).abs() > cell * 0.5 {
        return false;
    }
    let bh = (band.y1 - band.y0).max(1.0);

    let mut prefix = String::with_capacity(br.col + BRANCH_WIDTH);
    for c in 0..br.col {
        let x = origin + c as f32 * cell;
        if crosses_band(vs, x, band, cell, line_h, bh) {
            prefix.push('│');
        } else {
            prefix.push(' ');
        }
    }
    prefix.push(if br.down { '├' } else { '└' });
    prefix.push_str("── ");

    let body = strip_decoration(rest);
    if body.trim().is_empty() {
        return false;
    }
    let new = if gutter.is_empty() {
        format!("{prefix}{body}")
    } else {
        // Right-align the number in its own gutter and pad to the tree's pixel
        // column, so the branch glyphs land where the screenshot draws them.
        let number = gutter.trim();
        let target = origin_col.max(number.chars().count() + 1);
        format!("{number:>width$} {prefix}{body}", width = target - 1)
    };
    if new == line.rendered() {
        return false;
    }
    // A numbered row keeps the blank the assembler recovered before the
    // gutter; a plain tree row starts at the branch column.
    if gutter.is_empty() {
        line.indent = 0;
    }
    line.text = new;
    true
}

fn crosses_band(vs: &[VSeg], x: f32, band: Band, cell: f32, line_h: f32, bh: f32) -> bool {
    let tol = (cell * 0.3).max(2.0);
    vs.iter().any(|v| {
        let vx = (v.x0 as f32 + v.x1 as f32) / 2.0;
        (vx - x).abs() <= tol
            && (v.y1 - v.y0) as f32 >= line_h * 0.5
            && span_overlap(v.y0 as f32, v.y1 as f32, band.y0, band.y1) >= bh * 0.6
    })
}

/// Split a leading `  12 ` line-number gutter from the rest of the row.
///
/// Only an ASCII number followed by whitespace counts; a bare digit at the
/// start of a code line (`2 + 2`) is not a gutter. Returns an empty prefix
/// when there is no number.
fn split_line_number(s: &str) -> (&str, &str) {
    let trimmed = s.trim_start();
    let indent = s.len() - trimmed.len();
    let digits = trimmed.chars().take_while(|c| c.is_ascii_digit()).count();
    if digits == 0 || digits > 5 {
        return ("", s);
    }
    let after = &trimmed[digits..];
    if !after.starts_with(' ') {
        return ("", s);
    }
    let spaces = after.chars().take_while(|c| *c == ' ').count();
    let split = indent + digits + spaces;
    (&s[..split], &s[split..])
}

/// Characters that can only be decoration at the start of a line.
fn is_decoration(c: char) -> bool {
    matches!(
        c,
        ' ' | '\t'
            | '|'
            | '-'
            | '+'
            | '`'
            | '│'
            | '├'
            | '└'
            | '─'
            | '┌'
            | '┐'
            | '┘'
            | '┤'
            | '┬'
            | '┴'
            | '┼'
    )
}

fn is_box_glyph(c: char) -> bool {
    matches!(
        c,
        '│' | '├' | '└' | '─' | '┌' | '┐' | '┘' | '┤' | '┬' | '┴' | '┼'
    )
}

/// Drop a leading run that reads as a decoration prefix.
///
/// A bare `-` or `--` is left alone (`-5`, `--flag` are payload, not drawing);
/// a run ending in a space or containing a box glyph / pipe is a prefix.
fn strip_decoration(s: &str) -> &str {
    let mut end = 0;
    for (i, ch) in s.char_indices() {
        if !is_decoration(ch) {
            break;
        }
        end = i + ch.len_utf8();
    }
    if end == 0 {
        return s;
    }
    let run = &s[..end];
    if run.ends_with(' ') || run.contains('|') || run.chars().any(is_box_glyph) {
        &s[end..]
    } else {
        s
    }
}

// ---------------------------------------------------------------------------
// Table reconstruction
// ---------------------------------------------------------------------------

/// Rebuild the horizontal rule rows the engine dropped, and splice them back
/// between the cell rows it did emit.
///
/// A framed table states its own skeleton in pixels: every stroke position and
/// every column boundary is exact. The model reliably keeps the cell text and
/// just as reliably drops the `┌──┬──┐` row — on `ascii-table` that was 185 of
/// 441 non-space characters (42%), i.e. the whole 0.58 `content_acc`. This pass
/// only runs when the table's data bands and text rows line up one-to-one and
/// are consecutive, so a reconstruction can never land on the wrong row.
fn rebuild_rule_rows(
    lines: &mut Vec<Line>,
    geom: &Geometry,
    hs: &[HSeg],
    cols: &[Column],
    cell: f32,
    line_h: f32,
) -> RuleRebuild {
    let Some(table) = find_table(geom, hs, cols, cell, line_h) else {
        return RuleRebuild::default();
    };
    let band_in_table = |b: &Band| b.y1 > table.y0 && b.y0 < table.y1;
    let is_rule_band = |b: &Band| table.rules_y.iter().any(|y| *y >= b.y0 && *y <= b.y1);

    // The rule rows themselves form bands; only the remaining bands carry text.
    let mut rule_bands = Vec::new();
    let mut data_bands = Vec::new();
    for (bi, band) in geom.bands.iter().enumerate() {
        if !band_in_table(band) {
            continue;
        }
        if is_rule_band(band) {
            rule_bands.push(bi);
        } else {
            data_bands.push(bi);
        }
    }
    if rule_bands.is_empty() || data_bands.is_empty() {
        return RuleRebuild::default();
    }
    // Idempotence: when the text already has a row on a rule band, the engine
    // (or the caller's ground truth) emitted the frame itself. Inserting a
    // second one would duplicate the whole table.
    let rule_has_text = rule_bands.iter().any(|&bi| {
        let band = geom.bands[bi];
        lines.iter().any(|l| {
            l.bbox
                .is_some_and(|b| span_overlap(b.y0, b.y1, band.y0, band.y1) > 0.0)
        })
    });
    if rule_has_text {
        return RuleRebuild::default();
    }

    // Match each data band to the text line whose bbox overlaps it most.
    let mut chosen: Vec<Option<usize>> = vec![None; data_bands.len()];
    for (li, line) in lines.iter().enumerate() {
        let Some(bbox) = line.bbox else { continue };
        let mut best: Option<(usize, f32)> = None;
        for (di, &bi) in data_bands.iter().enumerate() {
            let band = geom.bands[bi];
            let overlap = span_overlap(bbox.y0, bbox.y1, band.y0, band.y1);
            if overlap > 0.0 && best.map(|(_, o)| overlap > o).unwrap_or(true) {
                best = Some((di, overlap));
            }
        }
        if let Some((di, _)) = best {
            if chosen[di].is_none() {
                chosen[di] = Some(li);
            }
        }
    }
    if chosen.iter().any(Option::is_none) {
        return RuleRebuild::default();
    }
    let indices: Vec<usize> = chosen.iter().flatten().copied().collect();
    let li_min = *indices.iter().min().unwrap();
    let li_max = *indices.iter().max().unwrap();
    // Consecutive, monotone rows only: a blank line or an out-of-table row in
    // the middle means the mapping is not trustworthy.
    if indices.len() != li_max - li_min + 1 || !indices.windows(2).all(|w| w[1] == w[0] + 1) {
        return RuleRebuild::default();
    }

    let slots = table_slots(&table, cell);
    if slots.len() < 2 {
        return RuleRebuild::default();
    }

    // Walk the bands in order and emit either a fresh rule row or the matched
    // cell row. The table's own band range is contiguous, so a splice at the
    // first/last matched line keeps every row in place.
    let first = geom.bands.iter().position(band_in_table).unwrap();
    let last = geom.bands.iter().rposition(band_in_table).unwrap();
    let mut rebuilt: Vec<Line> = Vec::new();
    let mut data_at = 0usize;
    let mut rules_done = 0usize;
    let last_rule = rule_bands.len().saturating_sub(1);
    for band in &geom.bands[first..=last] {
        if is_rule_band(band) {
            let kind = if rules_done == 0 {
                RuleKind::Top
            } else if rules_done == last_rule {
                RuleKind::Bottom
            } else {
                RuleKind::Mid
            };
            rules_done += 1;
            rebuilt.push(rule_line(&slots, kind, *band));
        } else {
            rebuilt.push(lines[chosen[data_at].unwrap()].clone());
            data_at += 1;
        }
    }

    let seps = rebuilt.iter().filter(|l| is_rule_text(&l.text)).count();
    lines.splice(li_min..=li_max, rebuilt);
    RuleRebuild { rows: seps, seps }
}

fn table_slots(table: &Table, cell: f32) -> Vec<usize> {
    let mut slots: Vec<usize> = table
        .cols
        .iter()
        .map(|x| ((x - table.x0) / cell).round().max(0.0) as usize)
        .collect();
    slots.sort_unstable();
    slots.dedup();
    slots
}

fn rule_line(slots: &[usize], kind: RuleKind, band: Band) -> Line {
    Line {
        text: unicode_separator(slots, kind),
        indent: 0,
        conf: None,
        x0: Some(band.x0),
        bbox: Some(crate::geom::Rect::new(band.x0, band.y0, band.x1, band.y1)),
    }
}

fn unicode_separator(slots: &[usize], kind: RuleKind) -> String {
    let last = *slots.last().unwrap();
    let mut chars = vec!['─'; last + 1];
    let n = slots.len();
    for (i, &slot) in slots.iter().enumerate() {
        chars[slot] = match kind {
            RuleKind::Top if i == 0 => '┌',
            RuleKind::Top if i + 1 == n => '┐',
            RuleKind::Top => '┬',
            RuleKind::Mid if i == 0 => '├',
            RuleKind::Mid if i + 1 == n => '┤',
            RuleKind::Mid => '┼',
            RuleKind::Bottom if i == 0 => '└',
            RuleKind::Bottom if i + 1 == n => '┘',
            RuleKind::Bottom => '┴',
        };
    }
    chars.into_iter().collect()
}

fn is_rule_text(s: &str) -> bool {
    !s.is_empty()
        && s.chars()
            .all(|c| matches!(c, '─' | '┌' | '┐' | '┬' | '┴' | '┼' | '├' | '┤' | '└' | '┘'))
}

/// Merge per-line-box vertical glyphs into table columns. Terminal screenshots
/// draw `│` once per row, so the strokes are separated by the line gap rather
/// than continuous.
fn columns(vs: &[VSeg], line_h: f32, tol: f32) -> Vec<Column> {
    let mut segs: Vec<(f32, f32, f32)> = vs
        .iter()
        .filter(|v| (v.y1 - v.y0) as f32 >= line_h * 0.5)
        .map(|v| ((v.x0 as f32 + v.x1 as f32) / 2.0, v.y0 as f32, v.y1 as f32))
        .collect();
    segs.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(Ordering::Equal));

    let mut groups: Vec<Vec<(f32, f32, f32)>> = Vec::new();
    for s in segs {
        match groups.last_mut() {
            Some(g) if (s.0 - g[0].0).abs() <= tol => g.push(s),
            _ => groups.push(vec![s]),
        }
    }

    let gap = (line_h * 0.5).max(4.0);
    let mut out = Vec::new();
    for g in groups {
        let x = g.iter().map(|s| s.0).sum::<f32>() / g.len() as f32;
        let mut ys: Vec<(f32, f32)> = g.iter().map(|s| (s.1, s.2)).collect();
        ys.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(Ordering::Equal));
        let mut merged: Vec<(f32, f32)> = Vec::new();
        for (a, b) in ys {
            match merged.last_mut() {
                Some(last) if a - last.1 <= gap => last.1 = last.1.max(b),
                _ => merged.push((a, b)),
            }
        }
        for (a, b) in merged {
            if b - a >= line_h * 0.5 {
                out.push(Column { x, y0: a, y1: b });
            }
        }
    }
    out
}

fn find_table(
    geom: &Geometry,
    hs: &[HSeg],
    cols: &[Column],
    cell: f32,
    line_h: f32,
) -> Option<Table> {
    let w = geom.width as f32;
    let min_rule_w = (w * 0.25).max(cell * 6.0);
    let rules: Vec<HSeg> = hs
        .iter()
        .filter(|h| (h.x1 - h.x0) as f32 >= min_rule_w)
        .copied()
        .collect();
    if rules.len() < 2 {
        return None;
    }

    // Two unrelated horizontal rules elsewhere in the image must not define one
    // giant table: keep the largest group that shares a span.
    let mut best: Vec<HSeg> = Vec::new();
    for anchor in &rules {
        let span = (anchor.x1 - anchor.x0) as f32;
        let group: Vec<HSeg> = rules
            .iter()
            .copied()
            .filter(|h| {
                span_overlap(h.x0 as f32, h.x1 as f32, anchor.x0 as f32, anchor.x1 as f32)
                    >= 0.7 * span
            })
            .collect();
        if group.len() > best.len() {
            best = group;
        }
    }
    if best.len() < 2 {
        return None;
    }

    let x0 = best.iter().map(|h| h.x0).min()? as f32;
    let x1 = best.iter().map(|h| h.x1).max()? as f32;
    let y0 = best.iter().map(|h| h.y0).min()? as f32;
    let y1 = best.iter().map(|h| h.y1).max()? as f32;

    // Table borders span at least two rows; a single-row glyph stem that
    // happens to line up with a rule is text, not a column.
    let mut table_cols: Vec<f32> = cols
        .iter()
        .filter(|c| c.y1 - c.y0 >= line_h * 1.5)
        .filter(|c| c.x >= x0 - cell && c.x <= x1 + cell)
        .filter(|c| span_overlap(c.y0, c.y1, y0, y1) > 0.0)
        .map(|c| c.x)
        .collect();
    table_cols = cluster(&table_cols, (cell * 0.3).max(2.0));
    if table_cols.len() < 2 {
        return None;
    }
    // Two rules mapping onto one character column means the advance is wrong;
    // refuse rather than produce a lopsided table. Anchor on the table's own
    // left edge: `geom.left` may belong to text above the table (a `$` prompt),
    // which would shift every slot by a fraction of a cell.
    let mut mapped: Vec<i64> = table_cols
        .iter()
        .map(|x| ((x - x0) / cell).round() as i64)
        .collect();
    let n = mapped.len();
    mapped.sort_unstable();
    mapped.dedup();
    if mapped.len() != n || mapped.iter().any(|c| *c < 0) {
        return None;
    }

    Some(Table {
        cols: table_cols,
        rules_y: best
            .iter()
            .map(|h| (h.y0 as f32 + h.y1 as f32) / 2.0)
            .collect(),
        x0,
        y0,
        y1,
    })
}

fn rewrite_tables(
    lines: &mut [Line],
    nonblank: &[usize],
    geom: &Geometry,
    hs: &[HSeg],
    cols: &[Column],
    cell: f32,
    line_h: f32,
) -> (usize, usize) {
    // The conservative pass writes ASCII `+---+` and `|`. If the text already
    // carries real box glyphs, that is the engine doing the job; rewriting it
    // would replace correct drawing with a worse approximation.
    if lines.iter().any(|l| l.text.chars().any(is_box_glyph)) {
        return (0, 0);
    }
    let Some(table) = find_table(geom, hs, cols, cell, line_h) else {
        return (0, 0);
    };
    let mut rows = 0usize;
    let mut seps = 0usize;
    for (bi, &li) in nonblank.iter().enumerate() {
        let band = geom.bands[bi];
        if band.y1 < table.y0 || band.y0 > table.y1 {
            continue;
        }
        let is_rule = table.rules_y.iter().any(|y| *y >= band.y0 && *y <= band.y1);
        let line = &mut lines[li];

        if is_rule {
            if line.text.chars().all(is_decoration) {
                let sep = separator(&table.cols, geom.left, cell);
                if line.text != sep || line.indent != 0 {
                    line.text = sep;
                    line.indent = 0;
                    seps += 1;
                }
            }
            continue;
        }

        let mut chars: Vec<char> = line.text.chars().collect();
        let mut changed = false;
        for &cx in &table.cols {
            if !column_crosses(cols, cx, band, line_h) {
                continue;
            }
            let col = ((cx - geom.left) / cell).round();
            if col < 0.0 || col < line.indent as f32 {
                continue;
            }
            let i = col as usize - line.indent as usize;
            if i < chars.len() {
                match chars[i] {
                    '|' => {}
                    ' ' => {
                        chars[i] = '|';
                        changed = true;
                    }
                    // a real character sits on the rule: never overwrite it
                    _ => {}
                }
            } else {
                while chars.len() < i {
                    chars.push(' ');
                }
                chars.push('|');
                changed = true;
            }
        }
        if changed {
            line.text = chars.into_iter().collect();
            rows += 1;
        }
    }
    (rows, seps)
}

fn column_crosses(cols: &[Column], x: f32, band: Band, line_h: f32) -> bool {
    let bh = (band.y1 - band.y0).max(1.0);
    cols.iter().any(|c| {
        (c.x - x).abs() <= 3.0
            && span_overlap(c.y0, c.y1, band.y0, band.y1) >= bh * 0.5
            && c.y1 - c.y0 >= line_h * 0.5
    })
}

fn separator(cols: &[f32], left: f32, cell: f32) -> String {
    let mut at: Vec<usize> = cols
        .iter()
        .map(|x| ((x - left) / cell).round().max(0.0) as usize)
        .collect();
    at.sort_unstable();
    at.dedup();
    let Some(&last) = at.last() else {
        return String::new();
    };
    let mut chars = vec![' '; last + 1];
    for pair in at.windows(2) {
        for slot in chars.iter_mut().take(pair[1]).skip(pair[0] + 1) {
            *slot = '-';
        }
    }
    for &c in &at {
        chars[c] = '+';
    }
    chars.into_iter().collect()
}
