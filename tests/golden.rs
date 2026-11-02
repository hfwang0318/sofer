//! End-to-end checks of the layout layer, using the committed bench scenes.
//!
//! The engine is simulated by feeding the *ground-truth text* with no bounding
//! boxes — exactly the shape of a VLM's output. So these tests measure one thing:
//! can the geometry layer put the indentation back?

use std::path::Path;

use sofer::assemble::{assemble, Opts};
use sofer::geom::analyze;
use sofer::RawLine;

fn truth_lines(scene: &str) -> Vec<String> {
    let p = format!("bench/truth/{scene}.txt");
    let s = std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("{p}: {e}"));
    let mut v: Vec<String> = s.lines().map(|l| l.trim_end().to_string()).collect();
    while v.last().map(|l| l.is_empty()).unwrap_or(false) {
        v.pop();
    }
    v
}

fn run(scene: &str) -> (Vec<sofer::Line>, sofer::assemble::AssembleStats) {
    let img = image::open(Path::new(&format!("bench/images/{scene}@2x.png")))
        .unwrap_or_else(|e| panic!("{scene}: {e}"))
        .to_luma8();
    let geom = analyze(&img);
    let raw: Vec<RawLine> = truth_lines(scene)
        .iter()
        .map(|l| RawLine::plain(l.clone()))
        .collect();
    assemble(&raw, &geom, &Opts::default())
}

fn indent_of(s: &str) -> usize {
    s.chars().take_while(|c| *c == ' ').count()
}

/// Share of non-blank truth lines whose leading spaces survived the round trip.
fn indent_accuracy(scene: &str) -> f32 {
    let (out, _) = run(scene);
    let truth = truth_lines(scene);
    let t: Vec<&String> = truth.iter().filter(|l| !l.trim().is_empty()).collect();
    let o: Vec<&sofer::Line> = out.iter().filter(|l| !l.text.trim().is_empty()).collect();
    let n = t.len().min(o.len());
    assert!(n > 5, "{scene}: only {n} comparable lines");
    let mut hit = 0;
    for i in 0..n {
        if indent_of(t[i]) == o[i].indent as usize {
            hit += 1;
        } else {
            eprintln!(
                "MISMATCH {scene} line {i}: want {} got {} {:?} / {:?}",
                indent_of(t[i]),
                o[i].indent,
                t[i],
                o[i].text
            );
        }
    }
    hit as f32 / n as f32
}

#[test]
fn git_diff_indentation_is_recovered() {
    let acc = indent_accuracy("git-diff");
    assert!(acc >= 0.9, "git-diff indent accuracy {acc}");
}

#[test]
fn pytest_traceback_indentation_is_recovered() {
    let acc = indent_accuracy("pytest-traceback");
    assert!(acc >= 0.85, "pytest indent accuracy {acc}");
}

#[test]
fn blank_lines_do_not_shift_alignment() {
    // tree has one blank line and 14 ink rows; a naive zip would shift everything
    let (out, st) = run("tree");
    assert_eq!(st.mode, "bands");
    assert_eq!(st.bands, 14);
    let texts: Vec<&str> = out
        .iter()
        .map(|l| l.text.trim())
        .filter(|t| !t.is_empty())
        .collect();
    assert_eq!(texts[0], "~/proj $ tree -L 3 --dirsfirst src");
    assert_eq!(texts[1], "src");
    assert!(texts
        .iter()
        .any(|t| t.starts_with("\u{251c}\u{2500}\u{2500} cli")));
    assert_eq!(texts.last().unwrap(), &"3 directories, 8 files");
}

#[test]
fn gutter_code_column_comes_from_pixels() {
    let (out, st) = run("editor-rust");
    assert!(st.gutter, "editor-rust has a line-number gutter");
    let code: Vec<&sofer::Line> = out.iter().filter(|l| l.text.contains("map:")).collect();
    // the line number stays in the row's own text (2 leading spaces are the
    // gutter's indent); the code's indentation after it is recovered from the
    // second ink run inside the band, not from the model's spaces
    assert_eq!(code[0].rendered(), "  5     map: HashMap<K, V>,".trim_end());
}

#[test]
fn model_blank_lines_are_replaced_by_the_image_spacing() {
    let img = image::open("bench/images/tree@2x.png").unwrap().to_luma8();
    let geom = analyze(&img);
    let truth = truth_lines("tree");
    // simulate a model that inserts a blank after every row
    let raw: Vec<RawLine> = truth
        .iter()
        .flat_map(|l| [RawLine::plain(l.clone()), RawLine::plain("")])
        .collect();
    let (out, _) = assemble(&raw, &geom, &Opts::default());
    let nonblank = out.iter().filter(|l| !l.text.trim().is_empty()).count();
    let expected = truth.iter().filter(|l| !l.trim().is_empty()).count();
    assert_eq!(nonblank, expected, "no text row may be lost");
    let blanks = out.iter().filter(|l| l.text.trim().is_empty()).count();
    assert_eq!(blanks, 1, "tree has exactly one blank line in the image");
}

#[test]
fn soft_wrap_join_is_opt_in() {
    let img = image::open("bench/images/softwrap@2x.png")
        .unwrap()
        .to_luma8();
    let geom = analyze(&img);
    let raw: Vec<RawLine> = truth_lines("softwrap")
        .iter()
        .map(|l| RawLine::plain(l.clone()))
        .collect();
    let (plain, _) = assemble(&raw, &geom, &Opts::default());
    let joined_opts = Opts {
        join_wrap: true,
        ..Default::default()
    };
    let (joined, stj) = assemble(&raw, &geom, &joined_opts);
    assert!(joined.len() < plain.len(), "join should merge rows");
    assert!(stj.joined_wraps > 0);
}
