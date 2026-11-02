//! Vertical tiling for images that exceed the model's output budget.
//!
//! Why this exists: a 4K screenshot of a terminal holds ~56 rows of text. The
//! model does not fail because it cannot *see* them, it fails because the answer
//! does not fit in `max_tokens` — measured: 235-char log lines are 134 tokens
//! (1.75 chars/token), so `max_tokens=3072` runs out after ~23 rows and the reply
//! is silently truncated. Raising the budget is not a fix either: at 8192 the same
//! image came back with 68 rows for 56 real ones (over-generation).
//!
//! The rules:
//!   * cut **only inside a blank gap between two text rows** — never through a row
//!   * prefer a wide gap (a paragraph break) when one is available
//!   * give each tile one row of overlap on each side
//!   * stitch by **row-band index**, not by matching text: the model rewrites
//!     spacing (`INFO  worker` → `INFO worker`), so text matching is unsound
//!
//! Sizing comes from the output budget, not from a fixed pixel size.

use crate::geom::{Band, Geometry};

/// Roughly how many characters of punctuation-heavy text one token covers.
/// Measured with `/tokenize` on a 235-char log line: 134 tokens.
const CHARS_PER_TOKEN: f32 = 1.75;

/// Keep a tile small enough that the vision encoder does not eat the context.
const MAX_TILE_AREA_PX: u64 = 4_000_000;

const MIN_ROWS_PER_TILE: usize = 8;
const MAX_ROWS_PER_TILE: usize = 24;

#[derive(Debug, Clone, Copy, serde::Serialize)]
pub struct Tile {
    pub first_band: usize,
    pub last_band: usize,
    /// Horizontal extent of this tile's ink, in original-image pixels.
    ///
    /// A terminal centred on a 5K screen leaves huge empty margins on both
    /// sides; cropping them per tile gives the vision encoder a higher
    /// effective resolution without splitting columns across two requests.
    pub x0: u32,
    pub x1: u32,
    pub y0: u32,
    pub y1: u32,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct Plan {
    pub tiles: Vec<Tile>,
    pub rows_per_tile: usize,
    pub tokens_per_row: u32,
    pub reason: &'static str,
}

/// Token cost of one row, estimated from pixels alone.
pub fn tokens_per_row(geom: &Geometry, adv: f32) -> u32 {
    let mut widths: Vec<f32> = geom
        .bands
        .iter()
        .filter(|b| b.x1 > b.x0)
        .map(|b| (b.x1 - b.x0) / adv.max(1.0))
        .collect();
    widths.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let chars_per_row = widths.get(widths.len() / 2).copied().unwrap_or(0.0);
    (chars_per_row / CHARS_PER_TOKEN).ceil().max(1.0) as u32
}

/// Rows per tile that the output budget can afford.
pub fn rows_per_tile_for(geom: &Geometry, adv: f32, max_tokens: u32) -> usize {
    let tpr = tokens_per_row(geom, adv);
    let budget = (max_tokens as f32 * 0.9) as u32;
    (budget / tpr.max(1)).clamp(MIN_ROWS_PER_TILE as u32, MAX_ROWS_PER_TILE as u32) as usize
}

/// Decide whether (and how) to tile. `None` means "send the whole image".
pub fn plan(geom: &Geometry, adv: f32, max_tokens: u32) -> Option<Plan> {
    let bands = &geom.bands;
    if bands.len() < MIN_ROWS_PER_TILE || max_tokens == 0 {
        return None;
    }

    let rows_per_tile = rows_per_tile_for(geom, adv, max_tokens);
    let area = geom.width as u64 * geom.height as u64;
    let rows_over_budget = bands.len() > rows_per_tile;
    let area_over_budget = area > MAX_TILE_AREA_PX;
    if !rows_over_budget && !area_over_budget {
        return None;
    }

    // With many rows, also respect the area cap so the encoder stays cheap.
    let rows_by_area = if area_over_budget {
        let per_row = (geom.height as f32 / bands.len() as f32).max(1.0);
        (MAX_TILE_AREA_PX as f32 / (geom.width as f32 * per_row)) as usize
    } else {
        rows_per_tile
    };
    let step = rows_by_area
        .clamp(MIN_ROWS_PER_TILE, MAX_ROWS_PER_TILE)
        .min(rows_per_tile);
    if step >= bands.len() {
        return None;
    }
    Some(build(
        geom,
        step,
        tokens_per_row(geom, adv),
        if rows_over_budget {
            "output budget"
        } else {
            "tile area"
        },
    ))
}

/// Cut the bands into tiles of `step` rows, with one row of overlap on each side.
fn build(geom: &Geometry, step: usize, tokens_per_row: u32, reason: &'static str) -> Plan {
    let bands = &geom.bands;
    let n = bands.len();
    let step = step.max(1);
    // Half a line height of horizontal margin keeps the first/last glyph whole;
    // one line of vertical overlap already comes from the band indices.
    let pad_x = (geom.line_height * 0.5).max(2.0);
    let mut tiles = Vec::new();
    let mut i = 0;
    while i < n {
        let j = (i + step).min(n);
        let y0 = bands[i.saturating_sub(1)].y0 as u32;
        let y1 = bands[j.min(n - 1)].y1 as u32;
        let slice = &bands[i..j];
        let min_x = slice.iter().map(|b| b.x0).fold(f32::MAX, f32::min);
        let max_x = slice.iter().map(|b| b.x1).fold(0.0f32, f32::max);
        let x0 = (min_x - pad_x).floor().clamp(0.0, geom.width as f32) as u32;
        let x1 = (max_x + pad_x).ceil().clamp(0.0, geom.width as f32) as u32;
        tiles.push(Tile {
            first_band: i,
            last_band: j,
            x0,
            x1: x1.max(x0 + 1).min(geom.width),
            y0,
            y1,
        });
        i = j;
    }
    Plan {
        tiles,
        rows_per_tile: step,
        tokens_per_row,
        reason,
    }
}

/// Stitch per-tile outputs back into one line list, in row order.
///
/// Each tile carries one row of overlap on both sides, so exactly one line is
/// dropped at the leading and trailing edge — by position, never by text.
pub fn stitch(mut per_tile: Vec<Vec<crate::engine::RawLine>>) -> Vec<crate::engine::RawLine> {
    let n = per_tile.len();
    let mut out = Vec::new();
    for (k, lines) in per_tile.drain(..).enumerate() {
        let mut lines = lines;
        if k > 0 && !lines.is_empty() {
            lines.remove(0);
        }
        if k + 1 < n && !lines.is_empty() {
            lines.pop();
        }
        out.extend(lines);
    }
    // A tile occasionally re-emits the row that straddles the cut, which survives
    // the positional drop and shows up as a duplicate. Collapse adjacent identical
    // rows, comparing whitespace-insensitively (the model rewrites `INFO  x` to
    // `INFO x`, so an exact comparison would miss it).
    let mut deduped: Vec<crate::engine::RawLine> = Vec::with_capacity(out.len());
    for line in out {
        let key: String = line.text.split_whitespace().collect();
        if let Some(prev) = deduped.last() {
            let prev_key: String = prev.text.split_whitespace().collect();
            if !key.is_empty() && key == prev_key {
                continue;
            }
        }
        deduped.push(line);
    }
    deduped
}

/// A quick sanity check used by `--stats`: tiled output should be close to the
/// number of rows in the image.
pub fn looks_consistent(rows: usize, bands: &[Band]) -> bool {
    if bands.is_empty() {
        return true;
    }
    let ratio = rows as f32 / bands.len() as f32;
    (0.85..=1.15).contains(&ratio)
}

/// Force a split, used when the first attempt came back short.
///
/// The estimate said "it fits", but the model disagreed — so ignore the estimate
/// and make at least two tiles.
pub fn plan_forced(geom: &Geometry, adv: f32, max_tokens: u32) -> Option<Plan> {
    let bands = &geom.bands;
    if bands.len() < MIN_ROWS_PER_TILE * 2 {
        return None;
    }
    let step = rows_per_tile_for(geom, adv, max_tokens)
        .min(bands.len() / 2)
        .max(MIN_ROWS_PER_TILE);
    Some(build(
        geom,
        step,
        tokens_per_row(geom, adv),
        "short first pass",
    ))
}

/// Should we retry with tiling? First pass returned far fewer rows than the image
/// has, which is the signature of a truncated reply.
pub fn should_retry(rows: usize, bands: usize, already_tiled: bool) -> bool {
    !already_tiled && bands >= MIN_ROWS_PER_TILE && (rows as f32) < bands as f32 * 0.7
}

/// Is `candidate` a better row count than `current`?
pub fn is_better(candidate: usize, current: usize, bands: usize) -> bool {
    if bands == 0 {
        return false;
    }
    let d = |r: usize| ((r as f32 / bands as f32) - 1.0).abs();
    d(candidate) < d(current)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geom::Band;

    fn geom(rows: usize, width: u32, height: u32) -> Geometry {
        let lh = height as f32 / rows as f32;
        let bands = (0..rows)
            .map(|i| Band {
                y0: i as f32 * lh,
                y1: (i + 1) as f32 * lh - 2.0,
                x0: 10.0,
                x1: width as f32 - 10.0,
            })
            .collect();
        Geometry {
            width,
            height,
            bands,
            left: 10.0,
            right: width as f32 - 10.0,
            line_height: lh,
            column_runs: Vec::new(),
        }
    }

    #[test]
    fn retry_triggers_only_on_a_short_first_pass() {
        assert!(should_retry(26, 56, false)); // huge-4k truncated
        assert!(!should_retry(56, 56, false));
        assert!(!should_retry(40, 56, false)); // 0.71, borderline -> no retry
        assert!(!should_retry(10, 56, true)); // already tiled
        assert!(!should_retry(1, 5, false)); // too few rows to judge
    }

    #[test]
    fn retry_keeps_the_row_count_closest_to_reality() {
        assert!(is_better(56, 26, 56)); // fixed
        assert!(!is_better(68, 56, 56)); // over-generation loses
        assert!(is_better(54, 26, 56));
        assert!(!is_better(0, 0, 0));
    }

    #[test]
    fn forced_plan_always_splits_at_least_twice() {
        let g = geom(30, 800, 900);
        let p = plan_forced(&g, 12.0, 3072).expect("forced");
        assert!(p.tiles.len() >= 2, "tiles {}", p.tiles.len());
        assert_eq!(p.reason, "short first pass");
        assert!(plan_forced(&geom(10, 800, 400), 12.0, 3072).is_none());
    }

    #[test]
    fn small_images_are_not_tiled() {
        let g = geom(20, 1200, 800);
        assert!(plan(&g, 16.0, 3072).is_none());
    }

    #[test]
    fn many_rows_get_split_by_the_output_budget() {
        // 56 rows, wide lines -> tokens/row is high -> small tiles
        let g = geom(56, 3940, 2145);
        let p = plan(&g, 16.0, 3072).expect("should tile");
        assert!(p.tokens_per_row > 100, "{}", p.tokens_per_row);
        assert!(
            (8..=24).contains(&p.rows_per_tile),
            "rows/tile {}",
            p.rows_per_tile
        );
        assert!(p.tiles.len() >= 3, "tiles {}", p.tiles.len());
        // tiles must cover every band exactly once
        assert_eq!(p.tiles.first().unwrap().first_band, 0);
        assert_eq!(p.tiles.last().unwrap().last_band, 56);
        for w in p.tiles.windows(2) {
            assert_eq!(w[0].last_band, w[1].first_band);
        }
    }

    #[test]
    fn short_lines_fit_more_rows_per_tile() {
        let g = geom(60, 800, 1600); // ~50 chars per row
        let p = plan(&g, 12.0, 3072).expect("should tile");
        assert!(p.rows_per_tile >= 16, "rows/tile {}", p.rows_per_tile);
    }

    #[test]
    fn stitching_drops_exactly_the_overlap() {
        // Contract: every tile also carries one row *after* its range, so a middle
        // tile is [prev_overlap, rows..., next_overlap]. Step = 2 rows.
        let mk = |s: &str| crate::engine::RawLine::plain(s);
        let out = stitch(vec![
            vec![mk("a"), mk("b"), mk("c")],          // bands 0..2 + next
            vec![mk("b"), mk("c"), mk("d"), mk("e")], // 2..4 + next
            vec![mk("d"), mk("e"), mk("f"), mk("g")], // 4..6 + next
            vec![mk("f"), mk("g")],                   // 6..7, last
        ]);
        let texts: Vec<&str> = out.iter().map(|l| l.text.as_str()).collect();
        assert_eq!(texts, vec!["a", "b", "c", "d", "e", "f", "g"]);
    }
}
