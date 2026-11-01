//! The OCR pipeline as a reusable library, not a CLI implementation detail.
//!
//! `main.rs` (CLI) and the desktop app both call [`Config::engine`] once and then
//! [`recognize`] per image. Everything that used to live in the command line —
//! tiling, retry, assembly, repair, language guessing — belongs to this module so
//! the two frontends cannot drift apart.
//!
//! ```text
//! image ─► engine ─► tile/retry ─► assemble ─► repair ─► Document
//! ```

use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::assemble::{assemble, AssembleStats, Opts};
use crate::engine::{llamacpp, tesseract, Engine, EngineOutput, Spec};
use crate::geom;
use crate::pool::Semaphore;
use crate::render::Format;
use crate::{boxdraw, lang, repair, tile, Document};

/// Which engine to build.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EngineMode {
    /// llama.cpp when it is reachable, tesseract otherwise.
    Auto,
    #[serde(rename = "llamacpp")]
    LlamaCpp,
    Tesseract,
    /// Deterministic engine for tests and for checking the layout layer.
    Mock,
}

impl Default for EngineMode {
    fn default() -> Self {
        Self::Auto
    }
}

/// Every knob the OCR core exposes, in the shape a UI can persist.
///
/// The field names are camelCase on the wire so a TypeScript settings object can
/// be passed through without a translation layer.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Config {
    /// engine selection; `auto` falls back from llama.cpp to tesseract
    pub engine: EngineMode,
    /// llama-server base URL (`None` = `OCR4CODE_LLAMA_URL` or the default)
    pub llama_url: Option<String>,
    /// prompt sent to the vision model (`None` = the measured default)
    pub prompt: Option<String>,
    /// output budget; also caps how many rows go into one tile
    pub max_tokens: u32,
    /// split tall images at row gaps instead of sending them whole
    pub tile: bool,
    /// tesseract language(s), e.g. `eng+chi_sim`
    pub tess_lang: String,
    /// tesseract page segmentation mode
    pub psm: u32,
    /// disable CV indentation reconstruction (debug view of raw engine output)
    pub no_geometry: bool,
    /// merge rows that look like terminal soft-wraps (heuristic, default off)
    pub join_wrap: bool,
    /// trust the model's spacing instead of stripping a detected line-number gutter
    pub keep_gutter: bool,
    /// rebuild box-drawing characters (trees, tables) from detected straight
    /// lines; off by default because it only helps when the engine dropped them
    pub box_art: bool,
    /// output shape; applied by the caller through [`crate::render::render`]
    pub format: Format,
    /// markdown fence language; `None` = guess from the text
    pub language: Option<String>,
    /// how many engine calls may be in flight (only matters when tiling)
    pub jobs: usize,

    // CLI-only escape hatches for testing the layout layer without a model.
    pub mock_text: Option<String>,
    pub mock_text_file: Option<PathBuf>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            engine: EngineMode::Auto,
            llama_url: None,
            prompt: None,
            max_tokens: 3072,
            tile: true,
            tess_lang: "eng".into(),
            psm: 6,
            no_geometry: false,
            join_wrap: false,
            keep_gutter: false,
            box_art: false,
            format: Format::Text,
            language: None,
            jobs: 1,
            mock_text: None,
            mock_text_file: None,
        }
    }
}

impl Config {
    /// Build the engine once per run. In `auto` mode the configured llama endpoint
    /// is health-checked (800 ms) before falling back to tesseract.
    pub fn engine(&self) -> Result<Box<dyn Engine>> {
        let spec = match self.engine {
            EngineMode::Mock => Spec::Mock {
                text: self.mock_text.clone(),
                text_file: self.mock_text_file.clone(),
            },
            EngineMode::Tesseract => Spec::Tesseract {
                lang: self.tess_lang.clone(),
                psm: self.psm,
            },
            EngineMode::LlamaCpp => Spec::LlamaCpp {
                url: self.llama_url.clone().unwrap_or_else(llamacpp::default_url),
                prompt: self
                    .prompt
                    .clone()
                    .unwrap_or_else(|| llamacpp::DEFAULT_PROMPT.to_string()),
                max_tokens: self.max_tokens,
            },
            EngineMode::Auto => {
                let url = self.llama_url.clone().unwrap_or_else(llamacpp::default_url);
                let prompt = self
                    .prompt
                    .clone()
                    .unwrap_or_else(|| llamacpp::DEFAULT_PROMPT.to_string());
                let llama = llamacpp::LlamaCpp::new(url, prompt, self.max_tokens);
                if llama.health() {
                    return Ok(Box::new(llama));
                }
                if tesseract::available() {
                    return Ok(Box::new(tesseract::Tesseract::new(
                        self.tess_lang.clone(),
                        self.psm,
                    )));
                }
                anyhow::bail!(
                    "no engine available: start `llama-server` (see `ocr4code doctor`) \
                     or install tesseract"
                )
            }
        };
        spec.build()
    }

    fn opts(&self) -> Opts {
        Opts {
            join_wrap: self.join_wrap,
            strip_gutter: !self.keep_gutter,
            no_geometry: self.no_geometry,
        }
    }
}

/// One engine-call budget shared by every image and every tile.
pub fn limiter(jobs: usize) -> Arc<Semaphore> {
    Arc::new(Semaphore::new(jobs.max(1)))
}

/// Diagnostics for `--stats`. The app can ignore this entirely.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Trace {
    pub tiling: Option<TilingTrace>,
    pub retry: Option<RetryTrace>,
    pub geometry: GeometryTrace,
    /// output rows vs. row bands; `false` means the reply looked truncated
    pub consistent: bool,
    pub rows: usize,
    pub bands: usize,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TilingTrace {
    pub tiles: usize,
    pub rows_per_tile: usize,
    pub tokens_per_row: u32,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RetryTrace {
    pub rows: usize,
    pub retry_rows: usize,
    pub bands: usize,
    pub tiles: usize,
    pub accepted: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GeometryTrace {
    pub mode: String,
    pub bands: usize,
    pub text_lines: usize,
    pub advance: f32,
    pub gutter: bool,
    pub joined_wraps: usize,
}

/// A recognised document plus the decisions that produced it.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Outcome {
    pub document: Document,
    pub trace: Trace,
}

/// Recognise one image with an already-built engine.
///
/// The call blocks (a VLM request or tesseract process). Callers that must stay
/// responsive run it on a worker thread; tiling uses `jobs` internally through
/// the shared `limiter`.
pub fn recognize(
    engine: &dyn Engine,
    path: &Path,
    cfg: &Config,
    limiter: &Semaphore,
) -> Result<Outcome> {
    let img = image::open(path).with_context(|| format!("opening image {}", path.display()))?;
    let luma = img.to_luma8();
    let geom = geom::analyze(&luma);
    let adv = geom.advance_prior();

    let plan = if cfg.tile {
        tile::plan(&geom, adv, cfg.max_tokens)
    } else {
        None
    };

    let mut out = run_engine(engine, path, &img, plan.as_ref(), cfg, limiter)?;
    let mut used = plan;
    let mut retry_trace = None;

    // The budget estimate can be wrong (tokenizers vary, models ramble). If the
    // first pass came back with far fewer rows than the image has, that is the
    // signature of a truncated reply — retry once, split, and keep whichever is
    // closer to the real row count.
    if cfg.tile && used.is_none() && tile::should_retry(out.lines.len(), geom.bands.len(), false)
    {
        if let Some(forced) = tile::plan_forced(&geom, adv, cfg.max_tokens) {
            let first_rows = out.lines.len();
            let tiles = forced.tiles.len();
            let retry = run_engine(engine, path, &img, Some(&forced), cfg, limiter)?;
            let retry_rows = retry.lines.len();
            let accepted = tile::is_better(retry_rows, first_rows, geom.bands.len());
            if accepted {
                out = retry;
                used = Some(forced);
            }
            retry_trace = Some(RetryTrace {
                rows: first_rows,
                retry_rows,
                bands: geom.bands.len(),
                tiles,
                accepted,
            });
        }
    }

    let tiling = used.as_ref().map(|p| TilingTrace {
        tiles: p.tiles.len(),
        rows_per_tile: p.rows_per_tile,
        tokens_per_row: p.tokens_per_row,
        reason: p.reason.to_string(),
    });

    let consistent = tile::looks_consistent(out.lines.len(), &geom.bands);
    let (mut lines, stats) = assemble(&out.lines, &geom, &cfg.opts());
    repair::normalize(&mut lines);
    if cfg.box_art {
        boxdraw::apply(&luma, &geom, stats.advance, &mut lines);
    }
    let text = lines
        .iter()
        .map(|l| l.rendered())
        .collect::<Vec<_>>()
        .join("\n");
    let language = match cfg.language.as_deref() {
        Some(l) => Some(l.to_string()),
        None => Some(lang::guess(&text).fence().to_string()),
    };
    let document = Document {
        engine: out.engine,
        source: path.display().to_string(),
        ms: out.ms,
        width: geom.width,
        height: geom.height,
        lines,
        language,
    };

    Ok(Outcome {
        trace: trace(out.lines.len(), &geom, &stats, tiling, retry_trace, consistent),
        document,
    })
}

fn trace(
    rows: usize,
    geom: &geom::Geometry,
    stats: &AssembleStats,
    tiling: Option<TilingTrace>,
    retry: Option<RetryTrace>,
    consistent: bool,
) -> Trace {
    Trace {
        tiling,
        retry,
        geometry: GeometryTrace {
            mode: stats.mode.to_string(),
            bands: stats.bands,
            text_lines: stats.text_lines,
            advance: stats.advance,
            gutter: stats.gutter,
            joined_wraps: stats.joined_wraps,
        },
        consistent,
        rows,
        bands: geom.bands.len(),
    }
}

/// Run the engine on the whole image, or tile by tile when a plan is given.
fn run_engine(
    engine: &dyn Engine,
    path: &Path,
    img: &image::DynamicImage,
    plan: Option<&tile::Plan>,
    cfg: &Config,
    limiter: &Semaphore,
) -> Result<EngineOutput> {
    let Some(p) = plan else {
        let _permit = limiter.acquire();
        return engine.recognize(path);
    };

    // Tiles are independent, so they run concurrently — bounded by the same
    // semaphore as whole images, which is what keeps `--jobs` a global budget
    // instead of "jobs per image" (3 images × 3 tiles would be 9 requests).
    // Wall clock for the whole tile phase: summing per-tile `ms` would over-report
    // by the concurrency factor (3 tiles in parallel are not 3× the time).
    let started = std::time::Instant::now();
    type TileOut = (u64, Vec<crate::engine::RawLine>);
    let results = crate::pool::map(&p.tiles, cfg.jobs.max(1), |k, t| -> Result<TileOut> {
        let crop = img.crop_imm(0, t.y0, img.width(), t.y1.saturating_sub(t.y0));
        static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let seq = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let tmp = std::env::temp_dir().join(format!(
            "ocr4code-tile-{}-{seq}-{k}.png",
            std::process::id()
        ));
        crop.save(&tmp).with_context(|| "writing tile")?;
        let out = {
            let _permit = limiter.acquire();
            engine.recognize(&tmp)?
        };
        let _ = std::fs::remove_file(&tmp);
        Ok((out.ms, out.lines))
    });

    let mut per_tile = Vec::with_capacity(p.tiles.len());
    for r in results {
        let (_tile_ms, lines) = r?;
        per_tile.push(lines);
    }
    let ms = started.elapsed().as_millis() as u64;
    Ok(EngineOutput {
        engine: format!("{}+{}tiles({})", engine.name(), p.tiles.len(), p.reason),
        lines: tile::stitch(per_tile),
        ms,
    })
}
