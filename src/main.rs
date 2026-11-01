//! CLI: `ocr4code ocr <image>` is the scriptable surface. The desktop shell in
//! `app/` calls the same `pipeline` module rather than re-implementing it.

use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use clap::{Parser, Subcommand, ValueEnum};


use ocr4code::cli_input::{self, Region};
use ocr4code::engine::{llamacpp, tesseract};
use ocr4code::pipeline::{self, Config, EngineMode};
use ocr4code::render::{self, Format};

#[derive(Parser)]
#[command(
    name = "ocr4code",
    version,
    about = "Offline OCR for code and terminal screenshots",
    long_about = "Turns a screenshot of code or a terminal into text that survives a paste: \
                  geometry from pixels, text from a local model, both platform-neutral."
)]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Recognise one or more images and print the text
    Ocr(OcrArgs),
    /// Show which engines are usable on this machine
    Doctor,
}

#[derive(clap::Args)]
struct OcrArgs {
    /// image file(s), a directory, or `-` for stdin: png / jpg / jpeg / webp / bmp / tiff
    #[arg(required = true)]
    images: Vec<PathBuf>,

    /// recurse into subdirectories when an input is a directory
    #[arg(long)]
    recursive: bool,

    /// crop each image to a pixel rectangle before recognition: X,Y,W,H
    #[arg(long, value_name = "X,Y,W,H")]
    region: Option<String>,

    /// engine to use
    #[arg(long, value_enum, default_value_t = EngineArg::Auto)]
    engine: EngineArg,

    /// output format
    #[arg(long, value_enum, default_value_t = Format::Text)]
    format: Format,

    /// fence language for markdown output (default: guessed)
    #[arg(long)]
    lang: Option<String>,

    /// write to a file instead of stdout
    #[arg(long, short)]
    out: Option<PathBuf>,

    /// do not reconstruct indentation from the image
    #[arg(long)]
    no_geometry: bool,

    /// merge terminal soft-wraps into one logical line (heuristic, off by default)
    #[arg(long)]
    join_wrap: bool,

    /// trust the model's own spacing (auto-detected for line-number gutters)
    #[arg(long)]
    keep_gutter: bool,

    /// print layout statistics to stderr
    #[arg(long)]
    stats: bool,

    /// how many images to recognise at the same time (1 = sequential)
    #[arg(long, short = 'j', default_value_t = 1)]
    jobs: usize,

    // ---- engine specific ----
    /// tesseract language(s), e.g. eng or eng+chi_sim
    #[arg(long, default_value = "eng")]
    tess_lang: String,

    /// tesseract page segmentation mode
    #[arg(long, default_value_t = 6)]
    psm: u32,

    /// llama-server base URL
    #[arg(long)]
    llama_url: Option<String>,

    /// prompt sent to the model
    #[arg(long)]
    prompt: Option<String>,

    #[arg(long, default_value_t = 3072)]
    max_tokens: u32,

    /// split tall images into row-band tiles (see docs/baseline.md §4②)
    #[arg(long, value_enum, default_value_t = TileArg::Auto)]
    tile: TileArg,

    /// mock engine: text file with the lines the engine "saw"
    #[arg(long)]
    mock_text_file: Option<PathBuf>,

    /// mock engine: literal text
    #[arg(long)]
    mock_text: Option<String>,
}

#[derive(Copy, Clone, PartialEq, Eq, ValueEnum)]
enum TileArg {
    Auto,
    Off,
}

#[derive(Copy, Clone, PartialEq, Eq, ValueEnum)]
enum EngineArg {
    Auto,
    Mock,
    Tesseract,
    #[value(name = "llamacpp")]
    LlamaCpp,
}

fn main() {
    if let Err(e) = run() {
        eprintln!("error: {e:#}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    match Cli::parse().cmd {
        Cmd::Doctor => doctor(),
        Cmd::Ocr(args) => ocr(args),
    }
}

/// One resolved input file plus the name the user typed. Temp files created for
/// stdin or a region must never show up in messages or the JSON `source`.
struct Input {
    path: PathBuf,
    label: String,
}

fn ocr(args: OcrArgs) -> Result<()> {
    let config = Config {
        engine: match args.engine {
            EngineArg::Auto => EngineMode::Auto,
            EngineArg::Mock => EngineMode::Mock,
            EngineArg::Tesseract => EngineMode::Tesseract,
            EngineArg::LlamaCpp => EngineMode::LlamaCpp,
        },
        llama_url: args.llama_url.clone(),
        prompt: args.prompt.clone(),
        max_tokens: args.max_tokens,
        tile: args.tile == TileArg::Auto,
        tess_lang: args.tess_lang.clone(),
        psm: args.psm,
        no_geometry: args.no_geometry,
        join_wrap: args.join_wrap,
        keep_gutter: args.keep_gutter,
        format: args.format,
        language: args.lang.clone(),
        jobs: args.jobs,
        mock_text: args.mock_text.clone(),
        mock_text_file: args.mock_text_file.clone(),
    };
    let region = args.region.as_deref().map(Region::parse).transpose()?;

    // Resolve every argument to a real file before touching the engine: a bad
    // pipe or an empty directory should fail fast, not after a model is loaded.
    let mut stdin_image: Option<cli_input::TempImage> = None;
    let mut inputs: Vec<Input> = Vec::new();
    for arg in &args.images {
        if cli_input::is_stdin(arg) {
            if stdin_image.is_some() {
                anyhow::bail!("stdin (`-`) can be given only once");
            }
            let temp = cli_input::TempImage::read_stdin(std::io::stdin().lock())?;
            inputs.push(Input {
                path: temp.path().to_path_buf(),
                label: "-".into(),
            });
            // Held until the run ends; dropping it deletes the spooled image.
            stdin_image = Some(temp);
        } else if arg.is_dir() {
            for path in cli_input::expand_directory(arg, args.recursive)? {
                inputs.push(Input {
                    label: path.display().to_string(),
                    path,
                });
            }
        } else {
            inputs.push(Input {
                label: arg.display().to_string(),
                path: arg.clone(),
            });
        }
    }

    let engine = config.engine()?;
    // A single budget for *engine calls* in flight, shared by images and by tiles.
    let limiter = pipeline::limiter(args.jobs);

    // One image failing must not take the rest down: collect the error, keep going,
    // report at the end and exit non-zero.
    let results: Vec<Result<String, (String, String)>> =
        ocr4code::pool::map(&inputs, args.jobs, |_, input| {
            // A region is materialised as a temp file because `recognize` takes a
            // path; the guard deletes it on the success *and* the error path.
            let cropped = match region {
                Some(r) => match cli_input::TempImage::crop(&input.path, r) {
                    Ok(temp) => Some(temp),
                    Err(e) => return Err((input.label.clone(), format!("{e:#}"))),
                },
                None => None,
            };
            let path = cropped
                .as_ref()
                .map(|t| t.path())
                .unwrap_or(input.path.as_path());
            match pipeline::recognize(engine.as_ref(), path, &config, &limiter) {
                Ok(mut outcome) => {
                    // never leak a temp path into `--stats` or the JSON `source`
                    outcome.document.source = input.label.clone();
                    if args.stats {
                        print_stats(Path::new(&input.label), &outcome);
                    }
                    let fence = config.language.clone();
                    Ok(render::render(&outcome.document, config.format, fence.as_deref()))
                }
                Err(e) => Err((input.label.clone(), format!("{e:#}"))),
            }
        });

    let mut failed = 0usize;
    let mut rendered: Vec<String> = Vec::with_capacity(results.len());
    for r in results {
        match r {
            Ok(text) => rendered.push(text),
            Err((label, msg)) => {
                failed += 1;
                eprintln!("error: {label}: {msg}");
            }
        }
    }
    if args.jobs > 1 {
        eprintln!(
            "done: {} ok, {} failed (jobs={})",
            rendered.len(),
            failed,
            args.jobs
        );
    }

    let joined = rendered.join("\n");
    match &args.out {
        Some(p) => {
            let mut f = std::fs::File::create(p)
                .with_context(|| format!("writing {}", p.display()))?;
            f.write_all(joined.as_bytes())?;
            f.write_all(b"\n")?;
        }
        None => {
            println!("{joined}");
        }
    }
    if failed > 0 {
        // the per-image error and the summary are already on stderr; a third line
        // from the generic handler would just be noise
        std::process::exit(1);
    }
    Ok(())
}

/// Reproduce the `--stats` report from the pipeline trace.
fn print_stats(path: &Path, outcome: &pipeline::Outcome) {
    let doc = &outcome.document;
    let trace = &outcome.trace;
    eprintln!(
        "[{}] {} lines, engine={}, {}ms",
        path.display(),
        doc.lines.len(),
        doc.engine,
        doc.ms
    );
    if let Some(r) = &trace.retry {
        eprintln!(
            "  retry: {}/{} rows -> {} tiles",
            r.rows, r.bands, r.tiles
        );
        if !r.accepted {
            eprintln!("  retry rejected: {} rows vs {}", r.retry_rows, r.rows);
        }
    }
    match &trace.tiling {
        Some(p) => eprintln!(
            "  tiling: {} tiles x {} rows ({} tok/row, {})",
            p.tiles, p.rows_per_tile, p.tokens_per_row, p.reason
        ),
        None => eprintln!("  tiling: off (fits one request)"),
    }
    if !trace.consistent {
        eprintln!(
            "  warning: output has {} rows for {} bands",
            trace.rows, trace.bands
        );
    }
    let g = &trace.geometry;
    eprintln!(
        "  geometry: mode={} bands={} text={} advance={:.1}px gutter={} wraps={}",
        g.mode, g.bands, g.text_lines, g.advance, g.gutter, g.joined_wraps
    );
}

fn doctor() -> Result<()> {
    println!("ocr4code {}", env!("CARGO_PKG_VERSION"));
    println!();

    let llama_url = llamacpp::default_url();
    let llama = llamacpp::LlamaCpp::new(llama_url.clone(), llamacpp::DEFAULT_PROMPT.into(), 256);
    let llama_ok = llama.health();
    println!("[{}] llama.cpp server  {llama_url}", mark(llama_ok));
    if !llama_ok {
        println!("      start it with:");
        println!("        llama-server -m ~/models/GLM-OCR-GGUF/GLM-OCR-Q8_0.gguf \\");
        println!("          --mmproj ~/models/GLM-OCR-GGUF/mmproj-GLM-OCR-Q8_0.gguf \\");
        println!("          --host 127.0.0.1 --port 8099 -ngl 99 -c 32768 -np 3");
        println!("      or set OCR4CODE_LLAMA_URL");
    }

    let tess_ok = tesseract::available();
    println!("[{}] tesseract          (fallback, low-end machines)", mark(tess_ok));
    if !tess_ok {
        println!("      brew install tesseract   |   winget install UB-Mannheim.TesseractOCR");
    }
    println!();
    println!("geometry layer: always available (pure CV, no external deps)");
    Ok(())
}

fn mark(ok: bool) -> &'static str {
    if ok {
        "ok"
    } else {
        "--"
    }
}
