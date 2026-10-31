//! Dev probe for the engine manager. Not shipped in the app bundle.
//!
//!   cargo run --example engine_probe -- status
//!   cargo run --example engine_probe -- download
//!   cargo run --example engine_probe -- start
//!   cargo run --example engine_probe -- stop

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use ocr4code_app::engine::{Event, Manager, ModelQuality, Paths};

fn main() {
    let command = std::env::args().nth(1).unwrap_or_else(|| "status".into());
    let quality = match std::env::var("OCR4CODE_MODEL_QUALITY").as_deref() {
        Ok("low") => ModelQuality::Low,
        _ => ModelQuality::High,
    };
    let config = ocr4code_app::engine::ServerConfig {
        quality,
        ..Default::default()
    };
    let home = std::env::var("HOME").unwrap_or_default();
    let data = PathBuf::from(home).join("Library/Application Support/com.ocr4code.desktop");
    let runtime = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("runtime/macos-arm64");
    let paths = Paths {
        runtime,
        models: data.join("models/glm-ocr"),
        logs: data.join("logs"),
    };
    println!("models: {}", paths.models.display());

    let last = Arc::new(Mutex::new(Instant::now() - Duration::from_secs(10)));
    let printer = last.clone();
    let sink = Arc::new(move |event: Event| match event {
        Event::Download(p) => {
            let mut last = printer.lock().unwrap();
            if p.phase != "downloading" || last.elapsed() >= Duration::from_secs(2) {
                let pct = if p.total > 0 {
                    (p.downloaded as f64 / p.total as f64) * 100.0
                } else {
                    0.0
                };
                println!(
                    "[{}] {:?} {:.1}% {:.0}/{:.0} MB {:.1} MB/s {:?}",
                    p.phase,
                    p.file,
                    pct,
                    p.downloaded as f64 / 1e6,
                    p.total as f64 / 1e6,
                    p.bytes_per_sec as f64 / 1e6,
                    p.eta_secs
                );
                if let Some(message) = p.message {
                    println!("  {message}");
                }
                *last = Instant::now();
            }
        }
        Event::State(s) => println!("[server] {} {:?} {:?}", s.state, s.url, s.error),
    });

    let manager = Manager::new(paths, sink);
    match command.as_str() {
        "status" => {
            let status = manager.status("http://127.0.0.1:8099", quality);
            println!("{}", serde_json::to_string_pretty(&status).unwrap());
        }
        "download" => match manager.ensure_model(&ocr4code_app::engine::DownloadOptions {
            quality,
            ..Default::default()
        }) {
            Ok(model) => println!(
                "model ready: {} MB verified={} at {}",
                model.bytes / 1_000_000,
                model.verified,
                model.dir
            ),
            Err(err) => {
                eprintln!("download failed: {err}");
                std::process::exit(1);
            }
        },
        "bootstrap" => match manager.bootstrap(
            "",
            &ocr4code_app::engine::DownloadOptions {
                quality,
                ..Default::default()
            },
            config,
        ) {
            Ok(()) => println!("bootstrap complete: model verified + server ready"),
            Err(err) => {
                eprintln!("bootstrap failed: {err}");
                std::process::exit(1);
            }
        },
        "start" => match manager.ensure_ready("", config) {
            Ok(url) => {
                println!("healthy at {url}");
                std::thread::sleep(Duration::from_secs(2));
                manager.stop();
            }
            Err(err) => {
                eprintln!("start failed: {err}");
                std::process::exit(1);
            }
        },
        "stop" => {
            manager.stop();
            println!("stopped");
        }
        other => {
            eprintln!("unknown command: {other}");
            std::process::exit(2);
        }
    }
}
