//! Exercise the remote deployment lifecycle without the desktop UI.
//!
//! ```text
//! cargo run --manifest-path app/src-tauri/Cargo.toml --example remote_probe -- probe
//! cargo run --manifest-path app/src-tauri/Cargo.toml --example remote_probe -- deploy
//! ```
//!
//! Configuration comes from `OCR4CODE_REMOTE_*` environment variables; see
//! `docs/remote-testing.md` (or run `--help`). The example calls the same
//! `remote::Manager` the app uses, so probe, SSH, runtime install, model
//! download/upload, server start and the tunnel are all real.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use ocr4code_app::engine::{health_ok, ModelQuality};
use ocr4code_app::remote::{Accel, Event, Manager, RemoteConfig, Source};

fn main() {
    let mut args = std::env::args().skip(1);
    let action = args.next().unwrap_or_else(|| "probe".into());
    if matches!(action.as_str(), "-h" | "--help" | "help") {
        print_help();
        return;
    }
    if args.next().is_some() {
        eprintln!("未知参数；每个动作不接受位置参数");
        std::process::exit(2);
    }

    let cfg = config();
    let local_models = PathBuf::from(env_or("OCR4CODE_REMOTE_LOCAL_MODELS", "/nonexistent"));
    let last_phase = Arc::new(Mutex::new(String::new()));
    let sink = {
        let last_phase = last_phase.clone();
        Arc::new(move |event: Event| match event {
            Event::Progress(progress) => {
                let mut parts = vec![progress.phase.clone()];
                if let Some(message) = progress.message {
                    parts.push(message);
                }
                if let Some(file) = progress.file {
                    parts.push(file);
                }
                if progress.total > 0 {
                    parts.push(format!(
                        "{} / {} ({:.0}%)",
                        bytes(progress.downloaded),
                        bytes(progress.total),
                        progress.downloaded as f64 * 100.0 / progress.total as f64
                    ));
                }
                if let Some(eta) = progress.eta_secs {
                    parts.push(format!("ETA {eta}s"));
                }
                println!("[progress] {}", parts.join(" | "));
            }
            Event::State(state) => {
                let mut previous = last_phase.lock().unwrap();
                if *previous == state.phase {
                    return;
                }
                *previous = state.phase.clone();
                drop(previous);
                println!(
                    "[state] phase={} url={:?} tunnel={} deployed={} error={:?}",
                    state.phase, state.url, state.tunnel, state.deployed, state.error
                );
            }
        })
    };

    let manager = Manager::new(local_models, sink);
    manager.set_enabled(true);

    let result = match action.as_str() {
        "probe" => run_probe(&manager, &cfg),
        "deploy" => run_deploy(&manager, &cfg),
        "start" => run_start(&manager, &cfg),
        "stop" => {
            manager.stop_with(&cfg);
            println!("[stop] 已发送停止命令");
            Ok(())
        }
        other => {
            eprintln!("未知动作 {other:?}");
            std::process::exit(2);
        }
    };

    // Test-only escape hatch: leave the remote server running so a separate
    // `stop` invocation can be exercised. The tunnel is still closed by Drop.
    if std::env::var("OCR4CODE_TEST_KEEP").ok().as_deref() != Some("1") {
        manager.shutdown();
    }
    if let Err(err) = result {
        eprintln!("[failed] {err}");
        std::process::exit(1);
    }
}

fn run_probe(manager: &Manager, cfg: &RemoteConfig) -> Result<(), String> {
    let probe = manager.probe(cfg).map_err(|e| e.to_string())?;
    println!("[probe] {}", serde_json::to_string_pretty(&probe).unwrap());
    Ok(())
}

fn run_deploy(manager: &Manager, cfg: &RemoteConfig) -> Result<(), String> {
    let source = match env_or("OCR4CODE_REMOTE_SOURCE", "download").as_str() {
        "download" | "auto" => Source::Download,
        "upload" => Source::Upload,
        other => return Err(format!("OCR4CODE_REMOTE_SOURCE 只能是 download/upload，收到 {other}")),
    };
    manager.deploy(cfg, source).map_err(|e| e.to_string())?;
    println!("[deploy] 完成，source={source:?}");
    verify(manager)
}

fn run_start(manager: &Manager, cfg: &RemoteConfig) -> Result<(), String> {
    let url = manager.ensure_ready(cfg).map_err(|e| e.to_string())?;
    println!("[start] 隧道就绪：{url}");
    verify(manager)
}

fn verify(manager: &Manager) -> Result<(), String> {
    let url = manager
        .state()
        .url
        .ok_or_else(|| "管理器没有隧道地址".to_string())?;
    if !health_ok(&url, Duration::from_secs(3)) {
        return Err(format!("{url}/health 不可用"));
    }
    println!("[health] {url}/health -> 200");

    let image = std::env::var("OCR4CODE_TEST_IMAGE").ok();
    if let Some(image) = image {
        run_ocr(&url, Path::new(&image)).map_err(|e| e.to_string())?;
    } else {
        println!("[ocr] 跳过（未设置 OCR4CODE_TEST_IMAGE）");
    }
    Ok(())
}

fn run_ocr(url: &str, image: &Path) -> Result<(), Box<dyn std::error::Error>> {
    use ocr4code::engine::llamacpp::{LlamaCpp, DEFAULT_PROMPT};
    use ocr4code::pipeline::{recognize, Config, EngineMode};
    use ocr4code::pool::Semaphore;
    use ocr4code::render::{render, Format};

    let engine = LlamaCpp::new(url.to_string(), DEFAULT_PROMPT.into(), 3072);
    let cfg = Config {
        engine: EngineMode::LlamaCpp,
        llama_url: Some(url.to_string()),
        max_tokens: 3072,
        tile: false,
        ..Default::default()
    };
    let started = std::time::Instant::now();
    let outcome = recognize(&engine, image, &cfg, &Semaphore::new(1))?;
    println!(
        "[ocr] {} 行，耗时 {:.1}s",
        outcome.document.lines.len(),
        started.elapsed().as_secs_f64()
    );
    println!("-----8<-----");
    println!("{}", render(&outcome.document, Format::Text, None));
    println!("----->8-----");
    Ok(())
}

fn config() -> RemoteConfig {
    RemoteConfig {
        host: env_or("OCR4CODE_REMOTE_HOST", "127.0.0.1"),
        user: env_or("OCR4CODE_REMOTE_USER", "ocr"),
        ssh_port: parse_env("OCR4CODE_REMOTE_SSH_PORT", 2222),
        key_path: env_or("OCR4CODE_REMOTE_KEY", ""),
        dir: env_or("OCR4CODE_REMOTE_DIR", "~/.ocr4code"),
        server_port: parse_env("OCR4CODE_REMOTE_SERVER_PORT", 8099),
        local_port: parse_env("OCR4CODE_REMOTE_LOCAL_PORT", 0),
        accel: match env_or("OCR4CODE_REMOTE_ACCEL", "cpu").as_str() {
            "cpu" => Accel::Cpu,
            "vulkan" => Accel::Vulkan,
            "cuda" => Accel::Cuda,
            other => {
                eprintln!("未知 OCR4CODE_REMOTE_ACCEL={other}，按 auto 处理");
                Accel::Auto
            }
        },
        gpu_layers: parse_env("OCR4CODE_REMOTE_GPU_LAYERS", 0),
        quality: match env_or("OCR4CODE_REMOTE_QUALITY", "low").as_str() {
            "low" => ModelQuality::Low,
            "high" => ModelQuality::High,
            other => {
                eprintln!("未知 OCR4CODE_REMOTE_QUALITY={other}，按 low 处理");
                ModelQuality::Low
            }
        },
        source: Source::Auto,
        jobs: parse_env("OCR4CODE_REMOTE_JOBS", 3),
        max_tokens: parse_env("OCR4CODE_REMOTE_MAX_TOKENS", 3072),
        kv_cache_quant: parse_bool("OCR4CODE_REMOTE_KV_Q8", false),
        idle_sleep_secs: parse_env("OCR4CODE_REMOTE_IDLE_SLEEP", 300),
        use_mirror: parse_bool("OCR4CODE_REMOTE_MIRROR", false),
    }
}

fn env_or(key: &str, fallback: &str) -> String {
    std::env::var(key).unwrap_or_else(|_| fallback.to_string())
}

fn parse_env<T: std::str::FromStr>(key: &str, fallback: T) -> T {
    std::env::var(key)
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(fallback)
}

fn parse_bool(key: &str, fallback: bool) -> bool {
    match std::env::var(key).ok().as_deref() {
        Some("1" | "true" | "yes") => true,
        Some("0" | "false" | "no") => false,
        _ => fallback,
    }
}

fn bytes(value: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];
    let mut size = value as f64;
    let mut unit = 0;
    while size >= 1024.0 && unit + 1 < UNITS.len() {
        size /= 1024.0;
        unit += 1;
    }
    format!("{size:.1} {}", UNITS[unit])
}

fn print_help() {
    println!(
        "用法: remote_probe <probe|deploy|start|stop>\n\n\
         环境变量:\n\
         OCR4CODE_REMOTE_HOST          默认 127.0.0.1\n\
         OCR4CODE_REMOTE_USER          默认 ocr\n\
         OCR4CODE_REMOTE_SSH_PORT      默认 2222\n\
         OCR4CODE_REMOTE_KEY           私钥路径\n\
         OCR4CODE_REMOTE_DIR           默认 ~/.ocr4code\n\
         OCR4CODE_REMOTE_SERVER_PORT   默认 8099\n\
         OCR4CODE_REMOTE_LOCAL_PORT    默认 0（自动）\n\
         OCR4CODE_REMOTE_ACCEL         cpu/vulkan/cuda/auto，默认 cpu\n\
         OCR4CODE_REMOTE_GPU_LAYERS    默认 0\n\
         OCR4CODE_REMOTE_QUALITY       low/high，默认 low\n\
         OCR4CODE_REMOTE_SOURCE        deploy 时 download/upload，默认 download\n\
         OCR4CODE_REMOTE_LOCAL_MODELS  上传时的本地模型目录\n\
         OCR4CODE_TEST_IMAGE           通过隧道做一次真实 OCR\n\
         OCR4CODE_TEST_KEEP            1 = 退出时保留远端服务（测 stop 用）\n\
         OCR4CODE_REMOTE_MIRROR        1 使用 hf-mirror.com"
    );
}
