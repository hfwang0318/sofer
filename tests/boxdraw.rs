//! Synthetic-pixel tests for the opt-in box-drawing reconstruction.
//!
//! The scenes are drawn with the `image` crate only: a "glyph" is a small ink
//! block, a rule is a 2–3 px stroke. That is enough for the geometry layer to
//! find bands and for `boxdraw` to find the straight lines, without depending on
//! a font or on the committed bench scenes.

use image::{GrayImage, Luma};
use sofer::boxdraw;
use sofer::geom::{self, Geometry};
use sofer::pipeline::{self, Config, EngineMode};
use sofer::Line;

const BG: u8 = 255;
const INK: u8 = 0;
const CELL: u32 = 12;
const CAP: u32 = 8; // ink height of one text row
const STEP: u32 = 12; // line box height: CAP + a 4 px inter-line gap
const PAD: u32 = 10;

struct Scene {
    img: GrayImage,
}

impl Scene {
    fn new(w: u32, h: u32) -> Self {
        Self {
            img: GrayImage::from_pixel(w, h, Luma([BG])),
        }
    }

    fn rect(&mut self, x0: u32, y0: u32, x1: u32, y1: u32) {
        for y in y0..y1.min(self.img.height()) {
            for x in x0..x1.min(self.img.width()) {
                self.img.put_pixel(x, y, Luma([INK]));
            }
        }
    }

    fn hline(&mut self, x0: u32, x1: u32, y: u32, thick: u32) {
        self.rect(x0, y, x1, y + thick);
    }

    fn vline(&mut self, x: u32, y0: u32, y1: u32) {
        self.rect(x, y0, x + 2, y1);
    }

    /// Text stand-in: one 6 px block per non-space character. Blocks are
    /// narrower than any rule, so they never register as a horizontal stroke.
    fn label(&mut self, band_top: u32, col: u32, text: &str) {
        let x0 = PAD + col * CELL;
        for (k, ch) in text.chars().enumerate() {
            if ch == ' ' {
                continue;
            }
            let x = x0 + k as u32 * CELL;
            self.rect(x + 2, band_top, x + 8, band_top + CAP);
        }
    }
}

fn band_top(row: u32) -> u32 {
    PAD + row * STEP
}

/// ```text
/// src
/// ├── cli
/// │   ├── main.rs
/// │   └── args.rs
/// └── lib.rs
/// ```
fn tree_scene() -> GrayImage {
    let mut s = Scene::new(280, 96);
    s.label(band_top(0), 0, "src");

    let top = band_top(1);
    s.vline(PAD, top, top + CAP);
    s.hline(PAD, PAD + 3 * CELL, top + 3, 2);
    s.label(top, 4, "cli");

    let top = band_top(2);
    s.vline(PAD, top, top + CAP);
    let bx = PAD + 4 * CELL;
    s.vline(bx, top, top + CAP);
    s.hline(bx, bx + 3 * CELL, top + 3, 2);
    s.label(top, 8, "main.rs");

    let top = band_top(3);
    s.vline(PAD, top, top + CAP);
    let bx = PAD + 4 * CELL;
    // `└` only has a vertical stub above the branch
    s.vline(bx, top, top + CAP * 3 / 4);
    s.hline(bx, bx + 3 * CELL, top + 3, 2);
    s.label(top, 8, "args.rs");

    let top = band_top(4);
    s.vline(PAD, top, top + CAP * 3 / 4);
    s.hline(PAD, PAD + 3 * CELL, top + 3, 2);
    s.label(top, 4, "lib.rs");
    s.img
}

/// A five-row table with borders at columns 0, 8 and 16.
fn table_scene() -> GrayImage {
    let mut s = Scene::new(240, 80);
    for row in 0..5 {
        let top = band_top(row);
        for col in [0, 8, 16] {
            s.vline(PAD + col * CELL, top, top + CAP);
        }
        if row % 2 == 0 {
            s.hline(PAD, PAD + 16 * CELL + 2, top + 3, 3);
        } else {
            s.label(top, 1, "name");
            s.label(top, 9, "age");
        }
    }
    s.img
}

fn line(text: &str) -> Line {
    Line {
        text: text.to_string(),
        indent: 0,
        conf: None,
        x0: None,
        bbox: None,
    }
}

fn rendered(lines: &[Line]) -> Vec<String> {
    lines.iter().map(|l| l.rendered()).collect()
}

fn analyze(img: &GrayImage) -> Geometry {
    geom::analyze(img)
}

#[test]
fn horizontal_branch_becomes_tee() {
    let img = tree_scene();
    let g = analyze(&img);
    let mut lines: Vec<Line> = ["src", "cli", "main.rs", "args.rs", "lib.rs"]
        .iter()
        .map(|t| line(t))
        .collect();
    let stats = boxdraw::apply(&img, &g, 12.0, &mut lines);

    assert_eq!(rendered(&lines)[1], "├── cli");
    assert!(stats.tree_lines >= 1, "stats: {stats:?}");
}

#[test]
fn vertical_rule_becomes_pipe_prefix() {
    let img = tree_scene();
    let g = analyze(&img);
    let mut lines: Vec<Line> = ["src", "cli", "main.rs", "args.rs", "lib.rs"]
        .iter()
        .map(|t| line(t))
        .collect();
    boxdraw::apply(&img, &g, 12.0, &mut lines);

    // row 2 is one level below `cli`; column 0 carries the ancestor rule
    assert_eq!(rendered(&lines)[2], "│   ├── main.rs");
    assert!(rendered(&lines)[2].starts_with('│'));
}

#[test]
fn corner_becomes_elbow() {
    let img = tree_scene();
    let g = analyze(&img);
    let mut lines: Vec<Line> = ["src", "cli", "main.rs", "args.rs", "lib.rs"]
        .iter()
        .map(|t| line(t))
        .collect();
    boxdraw::apply(&img, &g, 12.0, &mut lines);

    // the vertical stops at this branch, so it is the last child
    assert_eq!(rendered(&lines)[3], "│   └── args.rs");
    assert_eq!(rendered(&lines)[4], "└── lib.rs");
}

#[test]
fn ascii_approximation_is_replaced_not_stacked() {
    let img = tree_scene();
    let g = analyze(&img);
    // what an engine that emits ASCII stand-ins would return
    let mut lines = vec![
        line("src"),
        line("|-- cli"),
        line("|   |-- main.rs"),
        line("|   `-- args.rs"),
        line("`-- lib.rs"),
    ];
    boxdraw::apply(&img, &g, 12.0, &mut lines);

    let out = rendered(&lines);
    assert_eq!(out[1], "├── cli");
    assert_eq!(out[2], "│   ├── main.rs");
    assert_eq!(out[4], "└── lib.rs");
}

#[test]
fn band_count_mismatch_is_left_alone() {
    let img = tree_scene();
    let g = analyze(&img);
    assert_eq!(g.bands.len(), 5);
    let mut lines = vec![line("src"), line("cli"), line("main.rs"), line("args.rs")];
    let before = rendered(&lines);
    let stats = boxdraw::apply(&img, &g, 12.0, &mut lines);

    assert_eq!(stats, boxdraw::BoxArtStats::default());
    assert_eq!(rendered(&lines), before);
}

#[test]
fn image_without_straight_lines_is_untouched() {
    let mut s = Scene::new(200, 70);
    for row in 0..3 {
        s.label(band_top(row), 0, "plain text");
    }
    let img = s.img;
    let g = analyze(&img);
    assert_eq!(g.bands.len(), 3);

    let mut lines = vec![line("alpha"), line("beta"), line("gamma")];
    let before = rendered(&lines);
    let stats = boxdraw::apply(&img, &g, 12.0, &mut lines);

    assert_eq!(stats, boxdraw::BoxArtStats::default());
    assert_eq!(rendered(&lines), before);
}

#[test]
fn table_columns_and_separators_are_restored() {
    let img = table_scene();
    let g = analyze(&img);
    assert_eq!(g.bands.len(), 5);

    let mut lines = vec![
        line("-----"),
        line(" name    age"),
        line("-----"),
        line(" bob     42"),
        line("-----"),
    ];
    let stats = boxdraw::apply(&img, &g, 12.0, &mut lines);

    let out = rendered(&lines);
    assert_eq!(out.len(), 5, "box art must not add or drop lines");
    assert_eq!(out[0], "+-------+-------+");
    assert_eq!(out[2], "+-------+-------+");
    assert_eq!(out[4], "+-------+-------+");
    for row in [1, 3] {
        let chars: Vec<char> = out[row].chars().collect();
        assert_eq!(chars[0], '|', "left border: {:?}", out[row]);
        assert_eq!(chars[8], '|', "middle column: {:?}", out[row]);
        assert_eq!(chars[16], '|', "right border: {:?}", out[row]);
    }
    assert_eq!(stats.separators, 3);
    assert_eq!(stats.table_rows, 2);
}

#[test]
fn default_config_enables_box_art() {
    let cfg = Config::default();
    assert!(cfg.box_art, "box-art reconstruction is on by default");
}

#[test]
fn pipeline_reconstructs_a_tree_by_default_and_can_be_disabled() {
    let img = tree_scene();
    let path = std::env::temp_dir().join(format!("sofer-boxdraw-tree-{}.png", std::process::id()));
    img.save(&path).expect("write synthetic tree");

    let mut cfg = Config {
        engine: EngineMode::Mock,
        tile: false,
        ..Config::default()
    };
    cfg.mock_text = Some("src\ncli\nmain.rs\nargs.rs\nlib.rs\n".to_string());
    // The synthetic glyphs are shorter than the small-text threshold; without
    // this the pipeline would upscale and (correctly) skip box art at 1×.
    cfg.upscale_small = false;
    let engine = cfg.engine().expect("mock engine");
    let limiter = pipeline::limiter(1);

    // default: the pixels put the drawing back
    let drawn = pipeline::recognize(engine.as_ref(), &path, &cfg, &limiter)
        .expect("recognize with box art");
    assert_eq!(
        drawn.document.text(),
        "src\n├── cli\n│   ├── main.rs\n│   └── args.rs\n└── lib.rs"
    );

    // `--no-box-art` still gives the engine's text untouched
    cfg.box_art = false;
    let plain = pipeline::recognize(engine.as_ref(), &path, &cfg, &limiter)
        .expect("recognize without box art");
    let plain_lines: Vec<String> = plain.document.lines.iter().map(|l| l.rendered()).collect();
    assert_eq!(plain_lines, ["src", "cli", "main.rs", "args.rs", "lib.rs"]);
    assert!(!plain.document.text().contains('├'));

    let _ = std::fs::remove_file(&path);
}
