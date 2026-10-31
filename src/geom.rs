//! Pure-CV geometry: find text rows and their horizontal extents.
//!
//! Nothing here calls an OCR engine. The point (see docs §4.6) is that
//! indentation is a *geometric* property of the image, so it can be recovered on
//! any platform without Vision / WinRT / an engine that returns bounding boxes.

use image::GrayImage;

#[derive(Debug, Clone, Copy, serde::Serialize)]
pub struct Rect {
    pub x0: f32,
    pub y0: f32,
    pub x1: f32,
    pub y1: f32,
}

impl Rect {
    pub fn new(x0: f32, y0: f32, x1: f32, y1: f32) -> Self {
        Self { x0, y0, x1, y1 }
    }
    pub fn width(&self) -> f32 {
        (self.x1 - self.x0).max(0.0)
    }
    pub fn center_y(&self) -> f32 {
        (self.y0 + self.y1) / 2.0
    }
}

/// One visual row of ink as found by row projection.
#[derive(Debug, Clone, Copy, serde::Serialize)]
pub struct Band {
    pub y0: f32,
    pub y1: f32,
    pub x0: f32,
    pub x1: f32,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct Geometry {
    pub width: u32,
    pub height: u32,
    pub bands: Vec<Band>,
    /// leftmost ink column across the whole image
    pub left: f32,
    /// rightmost ink column across the whole image
    pub right: f32,
    /// median band height in px
    pub line_height: f32,
}

impl Geometry {
    /// Cell width guess from typography alone.
    ///
    /// `line_height` here is the *ink* height of a row (cap height + descenders),
    /// not the line box, so the ratio is ≈ 0.6 for typical monospace fonts.
    /// Only used when there is no text to measure ink width against.
    pub fn advance_prior(&self) -> f32 {
        (self.line_height * 0.6).max(1.0)
    }
}

/// Build an ink mask that works for dark-on-light and light-on-dark alike:
/// ink is whatever deviates from the background (median) colour by a margin.
pub fn ink_mask(img: &GrayImage) -> Vec<bool> {
    let (w, h) = img.dimensions();
    let mut hist = [0u32; 256];
    for p in img.pixels() {
        hist[p.0[0] as usize] += 1;
    }
    let total: u32 = w * h;
    let mut acc = 0u32;
    let mut bg = 0u8;
    for (v, &c) in hist.iter().enumerate() {
        acc += c;
        if acc * 2 >= total {
            bg = v as u8;
            break;
        }
    }
    // threshold from the deviation distribution (p90 * 0.35, clamped)
    let mut dev_hist = [0u32; 256];
    for p in img.pixels() {
        let d = (p.0[0] as i32 - bg as i32).unsigned_abs() as usize;
        dev_hist[d.min(255)] += 1;
    }
    // Threshold from the *bright* end of the distribution, not the middle: text is
    // a small fraction of the pixels, so a p90 would be dominated by background and
    // would let a faint window border / shadow count as ink (measured on a real
    // screenshot: the border shifted the column origin and cost every indent level).
    let mut acc = 0u32;
    let mut p90 = 0usize;
    for (d, &c) in dev_hist.iter().enumerate() {
        acc += c;
        if (acc as u64) * 10 >= (total as u64) * 9 {
            p90 = d;
            break;
        }
    }
    // Keep this low enough for thin strokes (underscores, dashes) to survive;
    // the window-border case is handled by cropping the chrome away, not by
    // raising the threshold (raising it lost `_` and `-` in real screenshots).
    let thr = ((p90 as f32) * 0.35).max(18.0).min(200.0) as i32;

    let mut mask = vec![false; (w * h) as usize];
    for (i, p) in img.pixels().enumerate() {
        mask[i] = ((p.0[0] as i32 - bg as i32).abs()) > thr;
    }
    mask
}

/// Row projection: contiguous runs of rows that contain ink.
pub fn band_rows(mask: &[bool], w: u32, h: u32) -> Vec<(u32, u32)> {
    let rows: Vec<u32> = (0..h)
        .map(|y| {
            let base = (y * w) as usize;
            mask[base..base + w as usize].iter().filter(|&&b| b).count() as u32
        })
        .collect();
    // A row counts as text if it carries a meaningful amount of ink. This must be
    // well above "a few pixels", because box-drawing vertical rules are designed to
    // connect between lines: with a low threshold every row inside a `tree` block
    // counts as ink and the whole block collapses into one band (measured on a real
    // screenshot: 16 lines -> 8 bands).
    let min_ink = ((w as f32 * 0.002) as u32).max(3);
    let mut out = Vec::new();
    let mut start: Option<u32> = None;
    for (y, &c) in rows.iter().enumerate() {
        let active = c >= min_ink;
        match (active, start) {
            (true, None) => start = Some(y as u32),
            (false, Some(s)) => {
                if y as u32 - s >= 3 {
                    out.push((s, y as u32));
                }
                start = None;
            }
            _ => {}
        }
    }
    if let Some(s) = start {
        if h - s >= 3 {
            out.push((s, h));
        }
    }
    out
}

fn band_bounds(mask: &[bool], w: u32, y0: u32, y1: u32) -> (u32, u32) {
    let mut x0 = u32::MAX;
    let mut x1 = 0u32;
    for y in y0..y1 {
        let base = (y * w) as usize;
        for x in 0..w {
            if mask[base + x as usize] {
                if x < x0 {
                    x0 = x;
                }
                if x > x1 {
                    x1 = x;
                }
            }
        }
    }
    if x0 == u32::MAX {
        (0, 0)
    } else {
        (x0, x1 + 1)
    }
}

pub fn analyze(img: &GrayImage) -> Geometry {
    let (w, h) = img.dimensions();
    let mask = ink_mask(img);
    let rows = band_rows(&mask, w, h);
    let bands: Vec<Band> = rows
        .iter()
        .map(|&(y0, y1)| {
            let (x0, x1) = band_bounds(&mask, w, y0, y1);
            Band {
                y0: y0 as f32,
                y1: y1 as f32,
                x0: x0 as f32,
                x1: x1 as f32,
            }
        })
        .filter(|b| b.x1 > b.x0)
        .collect();

    let left = bands.iter().map(|b| b.x0).fold(f32::MAX, f32::min);
    let right = bands.iter().map(|b| b.x1).fold(0.0, f32::max);
    let mut hs: Vec<f32> = bands.iter().map(|b| b.y1 - b.y0).collect();
    hs.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let line_height = hs.get(hs.len() / 2).copied().unwrap_or(0.0);

    Geometry {
        width: w,
        height: h,
        bands,
        left: if left.is_finite() { left } else { 0.0 },
        right,
        line_height,
    }
}

/// 1-D lattice fit: the cell width that best explains a set of x offsets.
///
/// A pure lattice fit is degenerate (any divisor of the true advance is perfect),
/// so the search is anchored on a prior and only allowed to move ±15%.
pub fn fit_advance(offsets: &[f32], prior: f32) -> f32 {
    if offsets.is_empty() || prior <= 0.0 {
        return prior.max(1.0);
    }
    let uniq: Vec<f32> = {
        let mut v: Vec<f32> = offsets.to_vec();
        v.sort_by(|a, b| a.partial_cmp(b).unwrap());
        v.dedup_by(|a, b| (*a - *b).abs() < 0.5);
        v
    };
    let mut best = prior;
    let mut best_cost = f32::MAX;
    let mut step = prior * 0.85;
    while step <= prior * 1.15 {
        let cost: f32 = uniq
            .iter()
            .map(|&x| {
                let r = x / step;
                (r - r.round()).powi(2)
            })
            .sum::<f32>()
            / uniq.len() as f32;
        if cost < best_cost - 1e-12 {
            best_cost = cost;
            best = step;
        }
        step += 0.02;
    }
    best.max(1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mask_from(rows: &[&str]) -> (Vec<bool>, u32, u32) {
        let h = rows.len() as u32;
        let w = rows[0].len() as u32;
        let mut m = vec![false; (w * h) as usize];
        for (y, r) in rows.iter().enumerate() {
            for (x, c) in r.chars().enumerate() {
                m[y * w as usize + x] = c == '#';
            }
        }
        (m, w, h)
    }

    #[test]
    fn bands_are_split_per_text_row() {
        // a row needs >= 4 px of ink to count (see min_ink): thin marks such as
        // box-drawing rules must not keep a gap "filled"
        let (m, w, h) = mask_from(&[
            "####", "####", "####", "    ", "    ", "####", "####", "####", "    ", "####", "####",
            "####",
        ]);
        let rows = band_rows(&m, w, h);
        assert_eq!(rows, vec![(0, 3), (5, 8), (9, 12)]);
    }

    #[test]
    fn band_bounds_track_left_offset() {
        let (m, w, h) = mask_from(&["    ####", "    ####", "    ####"]);
        let rows = band_rows(&m, w, h);
        let (x0, x1) = band_bounds(&m, w, rows[0].0, rows[0].1);
        assert_eq!((x0, x1), (4, 8));
    }

    #[test]
    fn advance_fit_recovers_true_cell_width() {
        // offsets are exact multiples of 16 px
        let offsets = vec![0.0, 16.0, 32.0, 48.0, 160.0, 176.0];
        let adv = fit_advance(&offsets, 15.0);
        assert!((adv - 16.0).abs() < 0.3, "got {adv}");
    }

    #[test]
    fn advance_fit_does_not_collapse_to_divisor() {
        // with a prior near the truth it must not pick 8 (a divisor of 16)
        let offsets = vec![0.0, 16.0, 32.0];
        let adv = fit_advance(&offsets, 15.6);
        assert!(adv > 14.0, "collapsed to {adv}");
    }
}
