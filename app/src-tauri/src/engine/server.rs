//! `llama-server` lifecycle: context sizing, port selection, spawn, health
//! checks, warm-up and shutdown.

use std::fs::{self, File};
use std::net::TcpListener;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use super::manifest::{model_files, ModelQuality};
use super::{io_err, Error, Manager, Phase};

impl Manager {
    /// Use an already-running external server, otherwise start the managed one.
    pub fn ensure_ready(&self, external_url: &str, config: ServerConfig) -> Result<String, Error> {
        if let Some(url) = self.online_url_if_healthy(config) {
            return Ok(url);
        }
        if health_ok(external_url, Duration::from_millis(600)) {
            return Ok(external_url.to_string());
        }
        if !self.runtime_ready() {
            return Err(Error::RuntimeMissing(
                "缺少内置 OCR 运行时；请使用完整安装包。".into(),
            ));
        }
        if !self.model_info(config.quality).installed {
            return Err(Error::SetupRequired);
        }
        self.start_managed(config)
    }

    fn online_url_if_healthy(&self, config: ServerConfig) -> Option<String> {
        let (url, online, current) = {
            let slot = self.slot.lock().unwrap();
            (slot.url.clone(), slot.phase == Phase::Online, slot.config)
        };
        if let Some(url) = url {
            if online && current == Some(config) && health_ok(&url, Duration::from_millis(500)) {
                return Some(url);
            }
        }
        None
    }

    pub fn start_managed(&self, config: ServerConfig) -> Result<String, Error> {
        let _lifecycle = self.lifecycle.lock().unwrap();

        if let Some(url) = self.online_url_if_healthy(config) {
            return Ok(url);
        }
        self.stop_child();

        let exe = self.runtime_executable().ok_or_else(|| {
            Error::RuntimeMissing("缺少内置 OCR 运行时。请重新安装完整版本。".into())
        })?;
        if !self.model_info(config.quality).installed {
            return Err(Error::SetupRequired);
        }

        let files = model_files(config.quality);
        let model = self.paths.models.join(files[0].name);
        let mmproj = self.paths.models.join(files[1].name);
        let port = pick_port();
        let url = format!("http://127.0.0.1:{port}");
        let log_path = self.log_path();
        if let Some(dir) = log_path.parent() {
            fs::create_dir_all(dir).map_err(io_err)?;
        }
        let log = File::create(&log_path).map_err(io_err)?;
        let log_err = log.try_clone().map_err(io_err)?;

        let bundled = self.bundled_runtime_ready();
        let args = server_args(&config, &model, &mmproj, port, bundled);

        let mut command = Command::new(&exe);
        command
            .current_dir(exe.parent().unwrap_or(Path::new(".")))
            .args(args)
            .stdout(Stdio::from(log))
            .stderr(Stdio::from(log_err));

        #[cfg(target_os = "macos")]
        if let Some(dir) = exe.parent() {
            command.env("DYLD_LIBRARY_PATH", dir);
        }
        #[cfg(target_os = "windows")]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
        }

        let child = command.spawn().map_err(|e| {
            Error::Server(format!("无法启动 llama-server（{}）：{e}", exe.display()))
        })?;
        write_pid(&self.pid_file, child.id());

        {
            let mut slot = self.slot.lock().unwrap();
            slot.child = Some(child);
            slot.url = Some(url.clone());
            slot.phase = Phase::Starting;
            slot.error = None;
            slot.managed = true;
            slot.config = Some(config);
        }
        self.emit_state();

        let started = Instant::now();
        let timeout = Duration::from_secs(180);
        loop {
            if self.cancel.load(Ordering::SeqCst) {
                self.stop_child();
                return Err(Error::Cancelled);
            }

            let exited = {
                let mut slot = self.slot.lock().unwrap();
                match slot.child.as_mut() {
                    Some(child) => match child.try_wait() {
                        Ok(Some(status)) => Some(Some(status)),
                        Ok(None) => None,
                        Err(_) => Some(None),
                    },
                    None => Some(None),
                }
            };
            if let Some(status) = exited {
                let tail = log_tail(&log_path);
                let detail = match status {
                    Some(s) => format!("llama-server 已退出：{s}"),
                    None => "llama-server 已退出".to_string(),
                };
                let message = if tail.is_empty() {
                    detail
                } else {
                    format!("{detail}\n\n{tail}")
                };
                self.fail(message.clone());
                return Err(Error::Server(message));
            }

            if health_ok(&url, Duration::from_millis(500)) {
                {
                    let mut slot = self.slot.lock().unwrap();
                    slot.phase = Phase::Online;
                    slot.error = None;
                }
                self.emit_state();
                return Ok(url);
            }

            if started.elapsed() > timeout {
                self.stop_child();
                let tail = log_tail(&log_path);
                let message = format!(
                    "llama-server 在 180 秒内未就绪{}",
                    if tail.is_empty() {
                        String::new()
                    } else {
                        format!("\n\n{tail}")
                    }
                );
                self.fail(message.clone());
                return Err(Error::Server(message));
            }
            std::thread::sleep(Duration::from_millis(350));
        }
    }

    fn fail(&self, message: String) {
        let _ = fs::remove_file(&self.pid_file);
        {
            let mut slot = self.slot.lock().unwrap();
            slot.phase = Phase::Failed;
            slot.error = Some(message);
            slot.managed = true;
        }
        self.emit_state();
    }

    pub fn stop(&self) {
        let _lifecycle = self.lifecycle.lock().unwrap();
        self.stop_child();
    }

    /// Stop the managed server and wait for Windows to release the runtime
    /// directory before the updater starts the NSIS installer.
    pub fn prepare_update(&self) {
        cleanup_stale_server(&self.pid_file);
        self.stop();
        std::thread::sleep(Duration::from_millis(800));
    }

    fn stop_child(&self) {
        let child = {
            let mut slot = self.slot.lock().unwrap();
            let child = slot.child.take();
            slot.phase = Phase::Stopped;
            slot.url = None;
            slot.error = None;
            slot.managed = false;
            slot.config = None;
            child
        };
        if let Some(mut child) = child {
            let _ = child.kill();
            let _ = child.wait();
        }
        let _ = fs::remove_file(&self.pid_file);
        self.emit_state();
    }
}

/// `--sleep-idle-seconds` value that releases the model right after each
/// capture. It is a real second rather than a sentinel because 0 already means
/// "never release".
pub const IMMEDIATE_RELEASE_SECS: u64 = 1;

/// llama-server startup knobs derived from the persisted settings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ServerConfig {
    pub quality: ModelQuality,
    pub jobs: u32,
    pub max_tokens: u32,
    pub kv_cache_quant: bool,
    pub idle_sleep_secs: u64,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            quality: ModelQuality::High,
            jobs: 3,
            max_tokens: 3072,
            kv_cache_quant: true,
            idle_sleep_secs: if cfg!(target_os = "windows") {
                IMMEDIATE_RELEASE_SECS
            } else {
                300
            },
        }
    }
}

/// The measured 4K tile prompt is ~6.4k tokens; leave ~1.3k for framing and
/// rounding. This reproduces the old `-c 32768 -np 3` at the default
/// `jobs=3, max_tokens=3072` (3 × 11008 = 33024).
pub const PROMPT_TOKEN_BUDGET: u32 = 6656;
pub const CONTEXT_MARGIN: u32 = 1280;

pub fn llama_slots(jobs: u32) -> u32 {
    jobs.clamp(1, 3)
}

pub fn per_slot_ctx(max_tokens: u32) -> u32 {
    let raw = PROMPT_TOKEN_BUDGET
        .saturating_add(max_tokens)
        .saturating_add(CONTEXT_MARGIN);
    let rounded = raw.div_ceil(256) * 256;
    rounded.max(8192)
}

pub fn total_ctx(jobs: u32, max_tokens: u32) -> u32 {
    llama_slots(jobs) * per_slot_ctx(max_tokens)
}

fn write_pid(path: &Path, pid: u32) {
    if let Some(dir) = path.parent() {
        let _ = fs::create_dir_all(dir);
    }
    let _ = fs::write(path, pid.to_string());
}

pub(super) fn cleanup_stale_server(pid_file: &Path) {
    let Ok(raw) = fs::read_to_string(pid_file) else {
        return;
    };
    let Ok(pid) = raw.trim().parse::<u32>() else {
        let _ = fs::remove_file(pid_file);
        return;
    };
    #[cfg(unix)]
    {
        let output = Command::new("ps")
            .args(["-p", &pid.to_string(), "-o", "comm="])
            .output();
        if let Ok(output) = output {
            let comm = String::from_utf8_lossy(&output.stdout).to_lowercase();
            if comm.contains("llama-server") {
                let _ = Command::new("kill")
                    .arg("-TERM")
                    .arg(pid.to_string())
                    .status();
            }
        }
    }
    #[cfg(target_os = "windows")]
    {
        let filter = format!("PID eq {pid}");
        if let Ok(output) = Command::new("tasklist")
            .args(["/FI", &filter, "/FO", "CSV", "/NH"])
            .output()
        {
            let line = String::from_utf8_lossy(&output.stdout).to_lowercase();
            if line.contains("llama-server") {
                let _ = Command::new("taskkill")
                    .args(["/PID", &pid.to_string(), "/F"])
                    .status();
            }
        }
    }
    let _ = fs::remove_file(pid_file);
}

pub fn health_ok(url: &str, timeout: Duration) -> bool {
    if url.trim().is_empty() {
        return false;
    }
    ureq::get(&format!("{}/health", url.trim_end_matches('/')))
        .timeout(timeout)
        .call()
        .map(|response| response.status() == 200)
        .unwrap_or(false)
}

/// Smallest possible image (1×1 PNG): enough for a vision chat template to
/// accept the request without costing a real inference.
const WARM_UP_IMAGE: &str = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==";

/// Wake a server that released its model after `--sleep-idle-seconds`.
///
/// `/health` still answers while the model is asleep, so it cannot be used for
/// this. One chat request with `max_tokens: 1` forces the model back into
/// memory (~0.2–0.7 s per docs/memory-footprint.md). Best effort: the caller
/// runs it off the capture thread and ignores a failure.
pub fn warm_up(url: &str, timeout: Duration) -> bool {
    let url = url.trim().trim_end_matches('/');
    if url.is_empty() {
        return false;
    }
    let body = match serde_json::to_string(&warm_up_payload()) {
        Ok(body) => body,
        Err(_) => return false,
    };
    ureq::post(&format!("{url}/v1/chat/completions"))
        .timeout(timeout)
        .set("Content-Type", "application/json")
        .send_string(&body)
        .map(|response| response.status() == 200)
        .unwrap_or(false)
}

pub(super) fn warm_up_payload() -> serde_json::Value {
    serde_json::json!({
        "messages": [{
            "role": "user",
            "content": [
                {"type": "image_url", "image_url": {"url": WARM_UP_IMAGE}},
                {"type": "text", "text": "."}
            ]
        }],
        "max_tokens": 1,
        "temperature": 0.0,
        "stream": false
    })
}

pub(super) fn server_args(
    config: &ServerConfig,
    model: &Path,
    mmproj: &Path,
    port: u16,
    bundled: bool,
) -> Vec<String> {
    let slots = llama_slots(config.jobs);
    let ctx = total_ctx(config.jobs, config.max_tokens);
    let mut args = vec![
        "-m".into(),
        model.display().to_string(),
        "--mmproj".into(),
        mmproj.display().to_string(),
        "--host".into(),
        "127.0.0.1".into(),
        "--port".into(),
        port.to_string(),
        "-c".into(),
        ctx.to_string(),
        "-np".into(),
        slots.to_string(),
        "-ngl".into(),
        if cfg!(target_os = "macos") { "99" } else { "0" }.into(),
    ];
    // Flags added after the pinned b12018 runtime are only safe there; an
    // older system llama-server would exit on an unknown argument.
    if bundled {
        args.push("-cram".into());
        args.push("0".into());
        if config.idle_sleep_secs > 0 {
            args.push("--sleep-idle-seconds".into());
            args.push(config.idle_sleep_secs.to_string());
        }
    }
    if config.kv_cache_quant {
        args.push("-ctk".into());
        args.push("q8_0".into());
        args.push("-ctv".into());
        args.push("q8_0".into());
    }
    args
}

fn pick_port() -> u16 {
    for port in 18099..18_120 {
        if TcpListener::bind(("127.0.0.1", port)).is_ok() {
            return port;
        }
    }
    TcpListener::bind(("127.0.0.1", 0))
        .and_then(|listener| listener.local_addr())
        .map(|addr| addr.port())
        .unwrap_or(18099)
}

fn log_tail(path: &Path) -> String {
    let Ok(text) = fs::read_to_string(path) else {
        return String::new();
    };
    let trimmed = text.trim();
    let mut start = trimmed.len().saturating_sub(4000);
    // Do not cut a UTF-8 sequence in half.
    while start < trimmed.len() && !trimmed.is_char_boundary(start) {
        start += 1;
    }
    trimmed[start..].to_string()
}
