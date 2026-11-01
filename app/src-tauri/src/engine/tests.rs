//! Engine unit tests. They live in one module so each production submodule
//! stays focused; item visibility is widened to `pub(super)` for these tests.

use super::download::{build_agent, chunk_path, ParallelOutcome, PARALLEL_CONNECTIONS};
use super::manifest::{
    marker_ok, model_url_with_base, sha256_file, write_marker, MMPROJ, OFFICIAL_REVISION,
};
use super::server::{server_args, warm_up_payload};
use sha2::{Digest, Sha256};

use super::*;

use std::time::Instant;

fn arg_value<'a>(args: &'a [String], key: &str) -> Option<&'a str> {
    args.windows(2)
        .find(|pair| pair[0] == key)
        .map(|pair| pair[1].as_str())
}

#[test]
fn warm_up_needs_a_url() {
    assert!(!warm_up("", Duration::from_millis(200)));
    assert!(!warm_up("   ", Duration::from_millis(200)));
}

#[test]
fn warm_up_payload_asks_for_one_token_and_carries_an_image() {
    // A vision chat template rejects a text-only message, so the wake-up
    // request has to look like a real OCR call — just with a 1×1 image.
    let payload = warm_up_payload();
    assert_eq!(payload["max_tokens"], 1);
    assert_eq!(payload["temperature"], 0.0);
    assert_eq!(payload["stream"], false);
    let content = &payload["messages"][0]["content"];
    assert_eq!(content[0]["type"], "image_url");
    assert!(content[0]["image_url"]["url"]
        .as_str()
        .unwrap_or_default()
        .starts_with("data:image/png;base64,"));
    assert_eq!(content[1]["type"], "text");
}

#[test]
fn warm_up_gives_up_quickly_when_nothing_listens() {
    let started = Instant::now();
    assert!(!warm_up("http://127.0.0.1:1", Duration::from_millis(200)));
    assert!(started.elapsed() < Duration::from_secs(3));
}

#[test]
fn default_server_args_match_the_memory_plan() {
    let config = ServerConfig {
        quality: ModelQuality::Low,
        jobs: 3,
        max_tokens: 3072,
        kv_cache_quant: true,
        idle_sleep_secs: 300,
    };
    let args = server_args(
        &config,
        Path::new("/m.gguf"),
        Path::new("/mmproj.gguf"),
        18099,
        true,
    );
    assert_eq!(arg_value(&args, "-c"), Some("33024"));
    assert_eq!(arg_value(&args, "-np"), Some("3"));
    assert_eq!(arg_value(&args, "-cram"), Some("0"));
    assert_eq!(arg_value(&args, "--sleep-idle-seconds"), Some("300"));
    assert_eq!(arg_value(&args, "-ctk"), Some("q8_0"));
    assert_eq!(arg_value(&args, "-ctv"), Some("q8_0"));

    let lean = ServerConfig {
        jobs: 1,
        max_tokens: 512,
        kv_cache_quant: false,
        idle_sleep_secs: 0,
        ..config
    };
    let args = server_args(
        &lean,
        Path::new("/m.gguf"),
        Path::new("/mmproj.gguf"),
        18099,
        false,
    );
    assert_eq!(arg_value(&args, "-c"), Some("8448"));
    assert_eq!(arg_value(&args, "-np"), Some("1"));
    assert!(arg_value(&args, "-cram").is_none());
    assert!(arg_value(&args, "--sleep-idle-seconds").is_none());
    assert!(arg_value(&args, "-ctk").is_none());
}

#[test]
fn context_plan_matches_the_old_defaults() {
    assert_eq!(per_slot_ctx(3072), 11_008);
    assert_eq!(total_ctx(3, 3072), 33_024);
    assert_eq!(total_ctx(1, 3072), 11_008);
    assert_eq!(llama_slots(1), 1);
    assert_eq!(llama_slots(8), 3);
    assert!(per_slot_ctx(8192) >= 8192 + PROMPT_TOKEN_BUDGET);
}

#[test]
fn manifest_sizes_match_total() {
    for quality in [ModelQuality::High, ModelQuality::Low] {
        let files = model_files(quality);
        assert_eq!(
            model_total(quality),
            files.iter().map(|f| f.bytes).sum::<u64>()
        );
        assert!(model_total(quality) > 0);
        assert!(files.iter().all(|f| f.bytes > 0 && f.sha256.len() == 64));
        assert!(files
            .iter()
            .all(|f| !f.repo.is_empty() && !f.revision.is_empty()));
    }
    assert!(model_total(ModelQuality::High) > model_total(ModelQuality::Low));
}

#[test]
fn proxy_credentials_are_percent_encoded_for_the_url() {
    let url = compose_proxy_url("127.0.0.1:7890", "user@name", "p@ss:word/?#%")
        .unwrap()
        .unwrap();
    assert_eq!(
        url,
        "http://user%40name:p%40ss%3Aword%2F%3F%23%25@127.0.0.1:7890"
    );
    let normalized = normalize_proxy_url(&url);
    assert_eq!(normalized, "http://user@name:p@ss:word/?#%@127.0.0.1:7890");
    assert!(ureq::Proxy::new(&normalized).is_ok());

    assert_eq!(
        compose_proxy_url("http://127.0.0.1:7890", "", "").unwrap(),
        Some("http://127.0.0.1:7890".into())
    );
    assert!(compose_proxy_url("http://127.0.0.1:7890", "", "secret").is_err());
    assert!(compose_proxy_url("http://u:p@127.0.0.1:7890", "", "").is_err());
}

#[test]
fn proxy_credentials_are_percent_decoded() {
    let normalized = normalize_proxy_url("http://user:p%40ss@127.0.0.1:7890");
    assert_eq!(normalized, "http://user:p@ss@127.0.0.1:7890");
    assert!(ureq::Proxy::new(&normalized).is_ok());
    assert_eq!(
        normalize_proxy_url("http://user:p@ss@127.0.0.1:7890"),
        "http://user:p@ss@127.0.0.1:7890"
    );
    assert_eq!(
        normalize_proxy_url("http://127.0.0.1:7890"),
        "http://127.0.0.1:7890"
    );
}

#[test]
fn model_url_honours_the_settings_override() {
    let file = ModelFile {
        name: "a.gguf",
        bytes: 1,
        sha256: "",
        repo: "repo/model",
        revision: "rev",
    };
    assert_eq!(
        model_url_with_base(&file, Some("https://mirror.example/base/"), false),
        "https://mirror.example/base/a.gguf"
    );
    assert_eq!(
        model_url_with_base(&file, None, false),
        "https://huggingface.co/repo/model/resolve/rev/a.gguf"
    );
    assert_eq!(
        model_url_with_base(&file, None, true),
        "https://hf-mirror.com/repo/model/resolve/rev/a.gguf"
    );

    let low = model_files(ModelQuality::Low)[0];
    assert_eq!(
            model_url_with_base(&low, None, false),
            "https://huggingface.co/mradermacher/GLM-OCR-GGUF/resolve/11fad2c98e06d1ca82a1b5bc0c9137ee5758991b/GLM-OCR.Q4_K_M.gguf"
        );
}

#[test]
fn sha256_matches_a_known_value() {
    let dir = std::env::temp_dir().join(format!("sofer-sha-{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join("x");
    fs::write(&path, b"abc").unwrap();
    assert_eq!(
        sha256_file(&path).unwrap(),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn resumes_a_partial_download() {
    use std::io::{Read as _, Write as _};
    use std::net::TcpListener;
    use std::thread;

    let body: &'static [u8] = b"0123456789abcdefghijklmnopqrstuvwxyz";
    let half = body.len() / 2;
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = thread::spawn(move || {
        let (mut socket, _) = listener.accept().unwrap();
        let mut buf = [0u8; 4096];
        let n = socket.read(&mut buf).unwrap();
        let request = String::from_utf8_lossy(&buf[..n]).to_string();
        let start = request
            .lines()
            .find(|line| line.to_lowercase().starts_with("range:"))
            .and_then(|line| line.split('=').nth(1))
            .and_then(|value| value.trim_end_matches('-').parse::<usize>().ok())
            .unwrap_or(0);
        let chunk = &body[start..];
        let status = if start > 0 {
            "206 Partial Content"
        } else {
            "200 OK"
        };
        let header = format!(
                "HTTP/1.1 {status}\r\nContent-Length: {}\r\nContent-Range: bytes {}-{}/{}\r\nConnection: close\r\n\r\n",
                chunk.len(),
                start,
                body.len() - 1,
                body.len()
            );
        socket.write_all(header.as_bytes()).unwrap();
        socket.write_all(chunk).unwrap();
        request
    });

    let dir = std::env::temp_dir().join(format!("sofer-resume-{}-{}", std::process::id(), port));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let part = dir.join("test.bin.part");
    fs::write(&part, &body[..half]).unwrap();

    let digest = format!("{:x}", Sha256::digest(body));
    let sha: &'static str = Box::leak(digest.into_boxed_str());
    let spec = ModelFile {
        name: "test.bin",
        bytes: body.len() as u64,
        sha256: sha,
        repo: "test/repo",
        revision: "test-rev",
    };
    let manager = Manager::new(
        Paths {
            runtime: dir.join("runtime"),
            models: dir.clone(),
            logs: dir.join("logs"),
        },
        Arc::new(|_| {}),
    );
    manager
        .download_file_from(
            &format!("http://127.0.0.1:{port}/test.bin"),
            spec,
            &part,
            0,
            body.len() as u64,
        )
        .unwrap();

    assert_eq!(fs::read(&part).unwrap(), body);
    let request = server.join().unwrap();
    assert!(
        request
            .to_lowercase()
            .contains(&format!("range: bytes={half}-")),
        "request did not resume: {request}"
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn removing_one_quality_keeps_the_other_pack() {
    let dir = std::env::temp_dir().join(format!("sofer-remove-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    for file in model_files(ModelQuality::High) {
        fs::write(dir.join(file.name), b"x").unwrap();
    }
    let low = model_files(ModelQuality::Low);
    fs::write(dir.join(low[0].name), b"x").unwrap();
    let manager = Manager::new(
        Paths {
            runtime: dir.join("runtime"),
            models: dir.clone(),
            logs: dir.join("logs"),
        },
        Arc::new(|_| {}),
    );

    manager.remove_model(ModelQuality::High).unwrap();
    assert!(!dir.join(model_files(ModelQuality::High)[0].name).exists());
    assert!(dir.join(low[0].name).exists());
    assert!(dir.join(MMPROJ.name).exists());

    manager.remove_model(ModelQuality::Low).unwrap();
    assert!(!dir.join(low[0].name).exists());
    assert!(!dir.join(MMPROJ.name).exists());
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn legacy_marker_counts_only_for_high_quality() {
    let dir = std::env::temp_dir().join(format!("sofer-legacy-marker-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    fs::write(
        dir.join("verified.json"),
        format!(r#"{{"revision":"{OFFICIAL_REVISION}","generated_at":0}}"#),
    )
    .unwrap();
    assert!(marker_ok(&dir, ModelQuality::High));
    assert!(!marker_ok(&dir, ModelQuality::Low));
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn parallel_download_joins_eight_ranges() {
    use std::io::{Read as _, Write as _};
    use std::net::TcpListener;
    use std::sync::Arc;
    use std::thread;

    let body: Arc<Vec<u8>> = Arc::new((0..1024 * 1024).map(|index| (index % 251) as u8).collect());
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let port = listener.local_addr().unwrap().port();
    let server_body = body.clone();
    let server = thread::spawn(move || {
        for _ in 0..PARALLEL_CONNECTIONS {
            let (mut socket, _) = listener.accept().unwrap();
            let body = server_body.clone();
            thread::spawn(move || {
                let mut buf = [0u8; 8192];
                let n = socket.read(&mut buf).unwrap();
                let request = String::from_utf8_lossy(&buf[..n]).to_string();
                let range = request
                    .lines()
                    .find(|line| line.to_lowercase().starts_with("range:"))
                    .unwrap();
                let spec = range.split('=').nth(1).unwrap().trim();
                let (start, end) = spec.split_once('-').unwrap();
                let start: usize = start.parse().unwrap();
                let end: usize = end.parse().unwrap();
                let chunk = &body[start..=end];
                let header = format!(
                        "HTTP/1.1 206 Partial Content\r\nContent-Length: {}\r\nContent-Range: bytes {}-{}/{}\r\nConnection: close\r\n\r\n",
                        chunk.len(),
                        start,
                        end,
                        body.len()
                    );
                socket.write_all(header.as_bytes()).unwrap();
                socket.write_all(chunk).unwrap();
            });
        }
    });

    let dir = std::env::temp_dir().join(format!("sofer-parallel-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let part = dir.join("model.gguf.part");
    let spec = ModelFile {
        name: "model.gguf",
        bytes: body.len() as u64,
        sha256: "",
        repo: "test/repo",
        revision: "rev",
    };
    let manager = Manager::new(
        Paths {
            runtime: dir.join("runtime"),
            models: dir.clone(),
            logs: dir.join("logs"),
        },
        Arc::new(|_| {}),
    );
    let url = format!("http://127.0.0.1:{port}/model.gguf");
    let agent = build_agent(&url, None).unwrap();
    let outcome = manager
        .download_parallel(&agent, &url, spec, &part, 0, spec.bytes)
        .unwrap();
    assert_eq!(outcome, ParallelOutcome::Done);
    assert_eq!(fs::read(&part).unwrap(), *body);
    assert!(!chunk_path(&part, 0).exists());
    server.join().unwrap();
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn parallel_download_resumes_a_legacy_prefix() {
    use std::io::{Read as _, Write as _};
    use std::net::TcpListener;
    use std::sync::Arc;
    use std::thread;

    let body: Arc<Vec<u8>> = Arc::new((0..1024 * 1024).map(|index| (index % 239) as u8).collect());
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let port = listener.local_addr().unwrap().port();
    let server_body = body.clone();
    let server = thread::spawn(move || {
        for _ in 0..PARALLEL_CONNECTIONS - 1 {
            let (mut socket, _) = listener.accept().unwrap();
            let body = server_body.clone();
            thread::spawn(move || {
                let mut buf = [0u8; 8192];
                let n = socket.read(&mut buf).unwrap();
                let request = String::from_utf8_lossy(&buf[..n]).to_string();
                let range = request
                    .lines()
                    .find(|line| line.to_lowercase().starts_with("range:"))
                    .unwrap();
                let spec = range.split('=').nth(1).unwrap().trim();
                let (start, end) = spec.split_once('-').unwrap();
                let start: usize = start.parse().unwrap();
                let end: usize = end.parse().unwrap();
                let chunk = &body[start..=end];
                let header = format!(
                        "HTTP/1.1 206 Partial Content\r\nContent-Length: {}\r\nContent-Range: bytes {}-{}/{}\r\nConnection: close\r\n\r\n",
                        chunk.len(),
                        start,
                        end,
                        body.len()
                    );
                socket.write_all(header.as_bytes()).unwrap();
                socket.write_all(chunk).unwrap();
            });
        }
    });

    let dir = std::env::temp_dir().join(format!("sofer-parallel-prefix-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let part = dir.join("model.gguf.part");
    fs::write(&part, &body[..300 * 1024]).unwrap();
    let spec = ModelFile {
        name: "model.gguf",
        bytes: body.len() as u64,
        sha256: "",
        repo: "test/repo",
        revision: "rev",
    };
    let manager = Manager::new(
        Paths {
            runtime: dir.join("runtime"),
            models: dir.clone(),
            logs: dir.join("logs"),
        },
        Arc::new(|_| {}),
    );
    let url = format!("http://127.0.0.1:{port}/model.gguf");
    let agent = build_agent(&url, None).unwrap();
    let outcome = manager
        .download_parallel(&agent, &url, spec, &part, 0, spec.bytes)
        .unwrap();
    assert_eq!(outcome, ParallelOutcome::Done);
    assert_eq!(fs::read(&part).unwrap(), *body);
    server.join().unwrap();
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn marker_round_trip() {
    let dir = std::env::temp_dir().join(format!("sofer-marker-{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    assert!(!marker_ok(&dir, ModelQuality::High));
    write_marker(&dir, ModelQuality::High).unwrap();
    assert!(marker_ok(&dir, ModelQuality::High));
    assert!(!marker_ok(&dir, ModelQuality::Low));
    let _ = fs::remove_dir_all(&dir);
}
