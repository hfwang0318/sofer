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
use crate::engine::{llamacpp, tesseract, Engine, EngineOutput, ImageFormat, ImageInput, Spec};
use crate::geom;
use crate::pool::Semaphore;
use crate::preprocess::{self, Transform};
use crate::render::Format;
use crate::{boxdraw, lang, repair, tile, Document};

/// Which engine to build.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum EngineMode {
    /// llama.cpp when it is reachable, tesseract otherwise.
    #[default]
    Auto,
    #[serde(rename = "llamacpp")]
    LlamaCpp,
    Tesseract,
    /// macOS only: Apple Vision, the fast engine (~0.1 s/image, docs §4.2).
    Vision,
    /// Deterministic engine for tests and for checking the layout layer.
    Mock,
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
    /// llama-server base URL (`None` = `SOFER_LLAMA_URL` or the default)
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
    /// lines. On by default: the model reliably keeps the cell text and drops
    /// the drawing, and the pass is already conservative (it only rewrites a
    /// prefix that is confirmed by a stroke).
    pub box_art: bool,
    /// output shape; applied by the caller through [`crate::render::render`]
    pub format: Format,
    /// encoding used when an image has to be turned into bytes for the engine
    /// (every tile; whole images forward their original bytes as-is while this
    /// stays the default `Png`)
    pub image_format: ImageFormat,
    /// markdown fence language; `None` = guess from the text
    pub language: Option<String>,
    /// how many engine calls may be in flight (only matters when tiling)
    pub jobs: usize,
    /// decoded-pixel ceiling; `0` disables it. A small compressed file can
    /// decode into hundreds of megapixels and `geom` allocates per pixel.
    pub max_pixels: u64,
    /// crop the background margins before the model sees the image
    pub trim: bool,
    /// upscale screenshots whose text is too small for the vision encoder
    pub upscale_small: bool,
    /// estimate the ink background per block instead of once per frame
    pub adaptive_background: bool,

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
            box_art: true,
            format: Format::Text,
            image_format: ImageFormat::Png,
            language: None,
            jobs: 1,
            max_pixels: preprocess::DEFAULT_MAX_PIXELS,
            trim: true,
            upscale_small: true,
            adaptive_background: true,
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
            EngineMode::Vision => Spec::Vision,
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
                    "no engine available: start `llama-server` (see `sofer doctor`) \
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
            snap_indent: false,
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
    pub preprocess: PreprocessTrace,
    pub tiling: Option<TilingTrace>,
    pub retry: Option<RetryTrace>,
    pub geometry: GeometryTrace,
    pub clean: CleanTrace,
    pub box_art: boxdraw::BoxArtStats,
    /// output rows vs. row bands; `false` means the reply looked truncated
    pub consistent: bool,
    pub rows: usize,
    pub bands: usize,
}

/// What happened to the image before the engine saw it.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PreprocessTrace {
    /// The engine received the caller's original bytes untouched.
    pub forwarded: bool,
    /// Content-box crop applied to the model copy, `[x0, y0, width, height]`
    /// in normalized-original pixels.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub crop: Option<[u32; 4]>,
    /// Uniform upscale factor for small text (1.0 = none).
    pub scale: f32,
    /// Frame size after EXIF/alpha/pixel-cap normalization.
    pub pixels: u64,
}

/// What the engine-boundary cleanup changed before assembly.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CleanTrace {
    /// language tag of a Markdown fence the whole reply was wrapped in
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fence: Option<String>,
    /// identical lines collapsed from a runaway reply
    pub deduped_rows: usize,
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
    pub rebuilt_blanks: usize,
    pub snapped_indents: usize,
    pub suspicious_rows: usize,
}

/// A recognised document plus the decisions that produced it.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Outcome {
    pub document: Document,
    pub trace: Trace,
}

/// Everything the engine calls need, after normalization but before any
/// request. Geometry and tile coordinates always refer to `image`.
struct Prepared {
    /// Normalized original (orientation, alpha and pixel cap applied).
    image: image::DynamicImage,
    /// The model copy, when cropping or upscaling changed it. `None` means the
    /// model sees `image` itself, so no second full-frame buffer is allocated.
    model: Option<image::DynamicImage>,
    bytes: Vec<u8>,
    /// MIME for forwarding `bytes` verbatim; `None` forces a re-encode.
    source: Option<&'static str>,
    transform: Transform,
    /// Content box that was cropped for the model, if any.
    crop: Option<[u32; 4]>,
    luma: image::GrayImage,
    geom: geom::Geometry,
    adv: f32,
    plan: Option<tile::Plan>,
}

impl Prepared {
    fn model(&self) -> &image::DynamicImage {
        self.model.as_ref().unwrap_or(&self.image)
    }

    fn input(&self) -> ImageInput<'_> {
        match self.source {
            Some(mime) => ImageInput::with_source(self.model(), &self.bytes, mime),
            None => ImageInput::from_pixels(self.model()),
        }
    }

    /// Crop one tile in original-image coordinates onto the model copy.
    fn crop_tile(&self, t: &tile::Tile) -> image::DynamicImage {
        let model = self.model();
        let map = |value: f32, offset: u32, limit: u32, round_up: bool| -> u32 {
            let scaled = (value - offset as f32) * self.transform.scale;
            let scaled = if round_up {
                scaled.ceil()
            } else {
                scaled.floor()
            };
            scaled.clamp(0.0, limit as f32) as u32
        };
        let x0 = map(
            t.x0 as f32,
            self.transform.dx,
            model.width().saturating_sub(1),
            false,
        );
        let y0 = map(
            t.y0 as f32,
            self.transform.dy,
            model.height().saturating_sub(1),
            false,
        );
        let x1 = map(t.x1 as f32, self.transform.dx, model.width(), true);
        let y1 = map(t.y1 as f32, self.transform.dy, model.height(), true);
        let x1 = x1.max(x0 + 1).min(model.width());
        let y1 = y1.max(y0 + 1).min(model.height());
        model.crop_imm(x0, y0, x1 - x0, y1 - y0)
    }
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
    let prep = prepare(path, cfg)?;
    recognize_prepared(engine, &prep, path, cfg, limiter)
}

/// Like [`recognize`], but reuse an already-decoded image.
///
/// The desktop app decodes once for its thumbnail and blank check; decoding a
/// 4K screenshot a second time is pure latency and a second peak allocation.
/// The caller must pass the result of [`preprocess::decode`] so the EXIF
/// orientation matches what `prepare` would have done.
pub fn recognize_decoded(
    engine: &dyn Engine,
    path: &Path,
    decoded: preprocess::Decoded,
    cfg: &Config,
    limiter: &Semaphore,
) -> Result<Outcome> {
    let prep = prepare_decoded(path, decoded, cfg)?;
    recognize_prepared(engine, &prep, path, cfg, limiter)
}

fn recognize_prepared(
    engine: &dyn Engine,
    prep: &Prepared,
    path: &Path,
    cfg: &Config,
    limiter: &Semaphore,
) -> Result<Outcome> {
    let out = run_engine(engine, prep, prep.plan.as_ref(), cfg, limiter)?;
    let (out, used, retry_trace) =
        retry_if_truncated(engine, prep, cfg, limiter, out, prep.plan.clone())?;
    Ok(finish(out, used, retry_trace, prep, path, cfg))
}

/// Like [`recognize`], but report the model's text through `on_delta` while it
/// is still generating.
///
/// Only an image that fits one request streams: when the plan wants tiles, the
/// reply arrives out of order and the existing one-shot path is used instead.
/// A stream that fails before producing anything falls back to one request;
/// one that breaks after text was delivered is reported as an error, because
/// retrying would append a second copy to what the caller already printed.
pub fn recognize_streaming(
    engine: &dyn Engine,
    path: &Path,
    cfg: &Config,
    limiter: &Semaphore,
    on_delta: &mut dyn FnMut(&str),
) -> Result<Outcome> {
    let prep = prepare(path, cfg)?;
    streaming_prepared(engine, &prep, path, cfg, limiter, on_delta)
}

/// Like [`recognize_streaming`], but reuse an already-decoded image (see
/// [`recognize_decoded`]).
pub fn recognize_streaming_decoded(
    engine: &dyn Engine,
    path: &Path,
    decoded: preprocess::Decoded,
    cfg: &Config,
    limiter: &Semaphore,
    on_delta: &mut dyn FnMut(&str),
) -> Result<Outcome> {
    let prep = prepare_decoded(path, decoded, cfg)?;
    streaming_prepared(engine, &prep, path, cfg, limiter, on_delta)
}

fn streaming_prepared(
    engine: &dyn Engine,
    prep: &Prepared,
    path: &Path,
    cfg: &Config,
    limiter: &Semaphore,
    on_delta: &mut dyn FnMut(&str),
) -> Result<Outcome> {
    let out = if prep.plan.is_none() {
        let _permit = limiter.acquire();
        let input = prep.input();
        let mut emitted = false;
        match engine.recognize_stream_image(&input, cfg.image_format, &mut |delta| {
            emitted = true;
            on_delta(delta);
        }) {
            Ok(out) => out,
            Err(err) if !emitted => {
                eprintln!("stream: {err:#}; retrying without streaming");
                engine.recognize_image(&input, cfg.image_format)?
            }
            Err(err) => return Err(err.context("streaming interrupted mid-reply")),
        }
    } else {
        run_engine(engine, prep, prep.plan.as_ref(), cfg, limiter)?
    };

    let (out, used, retry_trace) =
        retry_if_truncated(engine, prep, cfg, limiter, out, prep.plan.clone())?;
    Ok(finish(out, used, retry_trace, prep, path, cfg))
}

/// Read and decode the image once, then normalize it for the engine.
fn prepare(path: &Path, cfg: &Config) -> Result<Prepared> {
    // Read the file once: decode for the geometry layer and keep the encoded
    // bytes for the engine, instead of letting the engine re-read the path.
    let bytes = std::fs::read(path).with_context(|| format!("reading image {}", path.display()))?;
    let decoded =
        preprocess::decode(&bytes).with_context(|| format!("decoding image {}", path.display()))?;
    Ok(build_prepared(decoded, bytes, cfg))
}

fn prepare_decoded(path: &Path, decoded: preprocess::Decoded, cfg: &Config) -> Result<Prepared> {
    // The bytes are still needed for the forward-unchanged fast path; reading
    // them again is a page-cache hit, unlike decoding the frame twice.
    let bytes = std::fs::read(path).with_context(|| format!("reading image {}", path.display()))?;
    Ok(build_prepared(decoded, bytes, cfg))
}

/// Normalize, analyse, and decide what the model should see.
///
/// The order matters: orientation and alpha must be fixed before `geom` looks
/// at luminance, while cropping and upscaling only touch the model copy and
/// leave the geometry coordinate space alone.
fn build_prepared(decoded: preprocess::Decoded, bytes: Vec<u8>, cfg: &Config) -> Prepared {
    let (image, alpha_changed) = preprocess::flatten_alpha(decoded.image);
    let (image, capped) = preprocess::cap_pixels(image, cfg.max_pixels);
    let luma = image.to_luma8();
    let geom = geom::analyze_with(&luma, cfg.adaptive_background);
    let adv = geom.advance_prior();
    let plan = if cfg.tile {
        tile::plan(&geom, adv, cfg.max_tokens)
    } else {
        None
    };

    let mut changed = decoded.reoriented || alpha_changed || capped;
    let mut model: Option<image::DynamicImage> = None;
    let mut transform = Transform::IDENTITY;
    let mut crop = None;

    if cfg.trim {
        // Half a cell horizontally keeps the first/last glyph whole; a quarter
        // of a line vertically keeps ascenders and descenders from being cut.
        let pad_x = (geom.line_height * 0.5).max(2.0);
        let pad_y = (geom.line_height * 0.25).max(2.0);
        if let Some(bounds) = preprocess::content_bounds(&geom, pad_x, pad_y) {
            if preprocess::trim_is_worthwhile(&image, bounds) {
                model = Some(image.crop_imm(bounds.x0, bounds.y0, bounds.width(), bounds.height()));
                transform.dx = bounds.x0;
                transform.dy = bounds.y0;
                crop = Some([bounds.x0, bounds.y0, bounds.width(), bounds.height()]);
                changed = true;
            }
        }
    }

    let base = model.as_ref().unwrap_or(&image);
    if let Some((upscaled, factor)) =
        preprocess::upscale_small(base, geom.line_height, cfg.upscale_small)
    {
        model = Some(upscaled);
        transform.scale = factor;
        changed = true;
    }

    // The original bytes may only go out when nothing above changed a pixel.
    // WebP/TIFF are always re-encoded: llama.cpp decodes images with stb_image,
    // which does not understand either format.
    let source = if changed {
        None
    } else {
        preprocess::forwardable_mime(decoded.format)
    };

    Prepared {
        image,
        model,
        bytes,
        source,
        transform,
        crop,
        luma,
        geom,
        adv,
        plan,
    }
}

/// The budget estimate can be wrong (tokenizers vary, models ramble). If the
/// first pass came back with far fewer rows than the image has, that is the
/// signature of a truncated reply — retry once, split, and keep whichever is
/// closer to the real row count.
fn retry_if_truncated(
    engine: &dyn Engine,
    prepared: &Prepared,
    cfg: &Config,
    limiter: &Semaphore,
    mut out: EngineOutput,
    mut used: Option<tile::Plan>,
) -> Result<(EngineOutput, Option<tile::Plan>, Option<RetryTrace>)> {
    let bands = prepared.geom.bands.len();
    let mut retry_trace = None;
    if cfg.tile && used.is_none() && tile::should_retry(out.lines.len(), bands, false) {
        if let Some(forced) = tile::plan_forced(&prepared.geom, prepared.adv, cfg.max_tokens) {
            let first_rows = out.lines.len();
            let tiles = forced.tiles.len();
            let retry = run_engine(engine, prepared, Some(&forced), cfg, limiter)?;
            let retry_rows = retry.lines.len();
            let accepted = tile::is_better(retry_rows, first_rows, bands);
            if accepted {
                out = retry;
                used = Some(forced);
            }
            retry_trace = Some(RetryTrace {
                rows: first_rows,
                retry_rows,
                bands,
                tiles,
                accepted,
            });
        }
    }
    Ok((out, used, retry_trace))
}

/// Assemble, repair and render the engine text into a document. Shared by the
/// one-shot and streaming paths so both produce identical output for the same
/// text.
fn finish(
    mut out: EngineOutput,
    used: Option<tile::Plan>,
    retry_trace: Option<RetryTrace>,
    prep: &Prepared,
    path: &Path,
    cfg: &Config,
) -> Outcome {
    let luma = &prep.luma;
    let geom = &prep.geom;
    let tiling = used.as_ref().map(|p| TilingTrace {
        tiles: p.tiles.len(),
        rows_per_tile: p.rows_per_tile,
        tokens_per_row: p.tokens_per_row,
        reason: p.reason.to_string(),
    });

    // Clean at the engine boundary, before geometry sees the text:
    //  * a Markdown fence the model wrapped the reply in would otherwise become
    //    two content lines (and a second fence when rendering Markdown);
    //  * zero-width marks and full-width twins change the text width per
    //    character while the ink stays the same, which biases every width
    //    comparison in `assemble`.
    let fence_lang = repair::strip_fence(&mut out.lines);
    // Only a reply that overshoots the image's row count is checked for
    // repetition: two identical log lines are content, a runaway is not.
    let deduped =
        if geom.bands.is_empty() || (out.lines.len() as f32) <= geom.bands.len() as f32 * 1.5 {
            0
        } else {
            repair::dedup_repeated(&mut out.lines)
        };
    repair::normalize_raw(&mut out.lines);

    let consistent = tile::looks_consistent(out.lines.len(), &geom.bands);
    let kind = repair::Kind::of(
        &out.lines
            .iter()
            .map(|l| l.text.as_str())
            .collect::<Vec<_>>(),
    );
    let mut opts = cfg.opts();
    // Indent snapping assumes the document *has* an indent unit; compiler and
    // terminal output decorates with arbitrary offsets instead.
    opts.snap_indent = kind == repair::Kind::Code;
    let (mut lines, stats) = assemble(&out.lines, geom, &opts);
    // Only at native resolution: when a 1× screenshot was upscaled for the
    // model, geometry still works in the original pixels, where a `|` in code
    // is as wide as a `│` branch and the stroke detector misfires (measured:
    // `real-editor-code@1x` 0.996 -> 0.643).
    let box_art = if cfg.box_art && prep.transform.scale == 1.0 {
        boxdraw::apply(luma, geom, stats.advance, &mut lines)
    } else {
        boxdraw::BoxArtStats::default()
    };
    let text = lines
        .iter()
        .map(|l| l.rendered())
        .collect::<Vec<_>>()
        .join("\n");
    let language = match cfg.language.as_deref() {
        Some(l) => Some(l.to_string()),
        // The model's own fence tag is a stronger signal than keyword counting.
        None => fence_lang
            .clone()
            .filter(|l| !l.is_empty())
            .or_else(|| Some(lang::guess(&text).fence().to_string())),
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

    Outcome {
        trace: trace(
            out.lines.len(),
            geom,
            &stats,
            PreprocessTrace {
                forwarded: prep.source.is_some(),
                crop: prep.crop,
                scale: prep.transform.scale,
                pixels: u64::from(prep.image.width()) * u64::from(prep.image.height()),
            },
            CleanTrace {
                fence: fence_lang,
                deduped_rows: deduped,
            },
            box_art,
            tiling,
            retry_trace,
            consistent,
        ),
        document,
    }
}

// The trace is a flat record of independent decisions; grouping them into a
// builder would add indirection without making the call site clearer.
#[allow(clippy::too_many_arguments)]
fn trace(
    rows: usize,
    geom: &geom::Geometry,
    stats: &AssembleStats,
    preprocess: PreprocessTrace,
    clean: CleanTrace,
    box_art: boxdraw::BoxArtStats,
    tiling: Option<TilingTrace>,
    retry: Option<RetryTrace>,
    consistent: bool,
) -> Trace {
    Trace {
        preprocess,
        tiling,
        retry,
        geometry: GeometryTrace {
            mode: stats.mode.to_string(),
            bands: stats.bands,
            text_lines: stats.text_lines,
            advance: stats.advance,
            gutter: stats.gutter,
            joined_wraps: stats.joined_wraps,
            rebuilt_blanks: stats.rebuilt_blanks,
            snapped_indents: stats.snapped_indents,
            suspicious_rows: stats.suspicious_rows,
        },
        clean,
        box_art,
        consistent,
        rows,
        bands: geom.bands.len(),
    }
}

/// Run the engine on the whole image, or tile by tile when a plan is given.
///
/// Crops are encoded in memory and handed to the engine as pixels, so the tile
/// path never writes to (or reads back from) the temp directory.
fn run_engine(
    engine: &dyn Engine,
    prepared: &Prepared,
    plan: Option<&tile::Plan>,
    cfg: &Config,
    limiter: &Semaphore,
) -> Result<EngineOutput> {
    let Some(p) = plan else {
        let _permit = limiter.acquire();
        return engine.recognize_image(&prepared.input(), cfg.image_format);
    };

    // Tiles are independent, so they run concurrently — bounded by the same
    // semaphore as whole images, which is what keeps `--jobs` a global budget
    // instead of "jobs per image" (3 images × 3 tiles would be 9 requests).
    // Wall clock for the whole tile phase: summing per-tile `ms` would over-report
    // by the concurrency factor (3 tiles in parallel are not 3× the time).
    let started = std::time::Instant::now();
    type TileOut = (u64, Vec<crate::engine::RawLine>);
    let results = crate::pool::map(&p.tiles, cfg.jobs.max(1), |_, t| -> Result<TileOut> {
        let crop = prepared.crop_tile(t);
        let out = {
            let _permit = limiter.acquire();
            engine.recognize_image(&ImageInput::from_pixels(&crop), cfg.image_format)?
        };
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Mutex;

    use image::{DynamicImage, Rgb, RgbImage};

    /// An in-memory engine: it records what it was handed and refuses to fall
    /// back to the path-based flow.
    struct MemoryEngine {
        formats: Mutex<Vec<ImageFormat>>,
        had_source: Mutex<Vec<bool>>,
        path_calls: AtomicUsize,
    }

    impl MemoryEngine {
        fn new() -> Self {
            Self {
                formats: Mutex::new(Vec::new()),
                had_source: Mutex::new(Vec::new()),
                path_calls: AtomicUsize::new(0),
            }
        }
    }

    impl Engine for MemoryEngine {
        fn name(&self) -> &str {
            "memory"
        }

        fn recognize(&self, _image: &Path) -> Result<EngineOutput> {
            self.path_calls.fetch_add(1, Ordering::Relaxed);
            Ok(EngineOutput {
                engine: "memory".into(),
                lines: vec![],
                ms: 0,
            })
        }

        fn recognize_image(
            &self,
            input: &ImageInput<'_>,
            format: ImageFormat,
        ) -> Result<EngineOutput> {
            self.formats.lock().unwrap().push(format);
            self.had_source.lock().unwrap().push(input.source.is_some());
            Ok(EngineOutput {
                engine: "memory".into(),
                lines: vec![],
                ms: 0,
            })
        }
    }

    fn sample() -> DynamicImage {
        DynamicImage::ImageRgb8(RgbImage::from_pixel(40, 30, Rgb([250, 250, 250])))
    }

    fn plan() -> tile::Plan {
        tile::Plan {
            tiles: vec![
                tile::Tile {
                    first_band: 0,
                    last_band: 1,
                    x0: 0,
                    x1: 40,
                    y0: 0,
                    y1: 10,
                },
                tile::Tile {
                    first_band: 1,
                    last_band: 2,
                    x0: 0,
                    x1: 40,
                    y0: 10,
                    y1: 20,
                },
                tile::Tile {
                    first_band: 2,
                    last_band: 3,
                    x0: 0,
                    x1: 40,
                    y0: 20,
                    y1: 30,
                },
            ],
            rows_per_tile: 2,
            tokens_per_row: 10,
            reason: "test",
        }
    }

    fn prepared(img: &DynamicImage, source: Option<(&'static str, Vec<u8>)>) -> Prepared {
        let luma = img.to_luma8();
        let geom = crate::geom::analyze(&luma);
        let (bytes, source) = match source {
            Some((mime, bytes)) => (bytes, Some(mime)),
            None => (Vec::new(), None),
        };
        Prepared {
            image: img.clone(),
            model: None,
            bytes,
            source,
            transform: Transform::IDENTITY,
            crop: None,
            luma,
            geom,
            adv: 1.0,
            plan: None,
        }
    }

    #[test]
    fn tiles_are_encoded_in_memory_and_never_written_to_disk() {
        let img = sample();
        let engine = MemoryEngine::new();
        let cfg = Config {
            tile: true,
            jobs: 1,
            image_format: ImageFormat::Jpeg { quality: 55 },
            ..Default::default()
        };
        let limiter = limiter(1);

        let out = run_engine(
            &engine,
            &prepared(&img, None),
            Some(&plan()),
            &cfg,
            &limiter,
        )
        .expect("tiled run");

        assert!(out.lines.is_empty());
        assert_eq!(
            engine.path_calls.load(Ordering::Relaxed),
            0,
            "the disk-backed recognize(path) path must not be used for tiles"
        );
        let formats = engine.formats.lock().unwrap();
        assert_eq!(formats.len(), 3, "one in-memory call per tile");
        assert!(formats
            .iter()
            .all(|f| *f == ImageFormat::Jpeg { quality: 55 }));
        assert!(
            engine.had_source.lock().unwrap().iter().all(|s| !s),
            "crops must not carry (or need) the source file bytes"
        );
    }

    #[test]
    fn a_whole_image_hands_the_engine_both_pixels_and_source_bytes() {
        let img = sample();
        let engine = MemoryEngine::new();
        let cfg = Config::default();
        let limiter = limiter(1);

        run_engine(
            &engine,
            &prepared(&img, Some(("image/png", b"encoded-source".to_vec()))),
            None,
            &cfg,
            &limiter,
        )
        .expect("whole image");

        assert_eq!(engine.path_calls.load(Ordering::Relaxed), 0);
        assert_eq!(
            engine.formats.lock().unwrap().as_slice(),
            &[ImageFormat::Png]
        );
        assert_eq!(engine.had_source.lock().unwrap().as_slice(), &[true]);
    }
}
