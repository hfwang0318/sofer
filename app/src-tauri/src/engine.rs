//! Bundled llama.cpp runtime + one-click model bootstrap.
//!
//! Two pieces of the OCR stack are *not* in the installer:
//!
//! * the vision weights (1.43 GB, MIT) — too large to ship and updated separately
//! * the llama-server *runtime* is bundled, so the user never installs llama.cpp
//!
//! This module owns the whole lifecycle: resumable download, SHA-256 verification,
//! spawning/stopping `llama-server`, health checks, and the events the setup UI
//! renders. It deliberately has no Tauri types so it can be tested standalone.

use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub const MODEL_NAME: &str = "GLM-OCR Q8_0";
pub const MODEL_LICENSE: &str = "MIT";
pub const MODEL_REVISION: &str = "65a42de1148dbed2297e922b5dbc7d9b70c36578";
pub const MODEL_TOTAL_BYTES: u64 = 1_434_837_056;
pub const RUNTIME_VERSION: &str = "llama.cpp b12018";

const HF_BASE: &str =
    "https://huggingface.co/ggml-org/GLM-OCR-GGUF/resolve/65a42de1148dbed2297e922b5dbc7d9b70c36578";

#[derive(Debug, Clone, Copy)]
pub struct ModelFile {
    pub name: &'static str,
    pub bytes: u64,
    pub sha256: &'static str,
}

/// Pinned to the same Hugging Face revision as `MODEL_REVISION`.
pub const MODEL_FILES: [ModelFile; 2] = [
    ModelFile {
        name: "GLM-OCR-Q8_0.gguf",
        bytes: 950_433_408,
        sha256: "45bc244a6446aff850521dc41f18bc8d7105ad5f0c2c8c28af04e7cc4f4d50b1",
    },
    ModelFile {
        name: "mmproj-GLM-OCR-Q8_0.gguf",
        bytes: 484_403_648,
        sha256: "9c4b58e33e316ed142eb5dcb41abec3844d3e6e5dc361ffb782c3fa9d175141f",
    },
];

pub fn model_url(name: &str) -> String {
    // Escape hatch for an internal mirror / offline staging server. The base
    // must include the resolve path, e.g.
    // https://mirror.example/ggml-org/GLM-OCR-GGUF/resolve/<revision>
    let base = std::env::var("OCR4CODE_MODEL_BASE")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| HF_BASE.to_string());
    format!("{}/{}", base.trim_end_matches('/'), name)
}

fn model_total() -> u64 {
    MODEL_FILES.iter().map(|f| f.bytes).sum()
}

#[derive(Debug, Clone)]
pub struct Paths {
    /// Directory containing the bundled `llama-server` and its libraries.
    pub runtime: PathBuf,
    /// `.../models/glm-ocr` (weights live here).
    pub models: PathBuf,
    /// Where the server writes its log.
    pub logs: PathBuf,
}

pub type EventSink = Arc<dyn Fn(Event) + Send + Sync>;

#[derive(Debug, Clone)]
pub enum Event {
    Download(DownloadProgress),
    State(ServerInfo),
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadProgress {
    /// downloading | verifying | starting | ready | error | cancelled
    pub phase: String,
    pub file: Option<String>,
    pub downloaded: u64,
    pub total: u64,
    pub bytes_per_sec: u64,
    pub eta_secs: Option<u64>,
    pub message: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeInfo {
    pub bundled: bool,
    pub system: bool,
    pub version: Option<String>,
    pub platform: String,
    pub path: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelInfo {
    pub installed: bool,
    pub verified: bool,
    pub bytes: u64,
    pub total: u64,
    pub dir: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerInfo {
    /// stopped | starting | online | failed
    pub state: String,
    pub url: Option<String>,
    pub managed: bool,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SetupInfo {
    pub needed: bool,
    pub download_bytes: u64,
    pub model_name: String,
    pub license: String,
    pub runtime_ready: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub llama: bool,
    pub tesseract: bool,
    pub llama_url: String,
    pub runtime: RuntimeInfo,
    pub model: ModelInfo,
    pub server: ServerInfo,
    pub setup: SetupInfo,
}

#[derive(Debug)]
pub enum Error {
    SetupRequired,
    Cancelled,
    RuntimeMissing(String),
    Download(String),
    Server(String),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::SetupRequired => write!(f, "本地 OCR 模型尚未安装"),
            Error::Cancelled => write!(f, "设置已取消"),
            Error::RuntimeMissing(m) => write!(f, "{m}"),
            Error::Download(m) => write!(f, "{m}"),
            Error::Server(m) => write!(f, "{m}"),
        }
    }
}

impl std::error::Error for Error {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    Stopped,
    Starting,
    Online,
    Failed,
}

impl Phase {
    fn as_str(self) -> &'static str {
        match self {
            Phase::Stopped => "stopped",
            Phase::Starting => "starting",
            Phase::Online => "online",
            Phase::Failed => "failed",
        }
    }
}

#[derive(Default)]
struct Slot {
    child: Option<Child>,
    url: Option<String>,
    phase: Phase,
    error: Option<String>,
    managed: bool,
}

impl Default for Phase {
    fn default() -> Self {
        Phase::Stopped
    }
}

pub struct Manager {
    paths: Paths,
    sink: EventSink,
    slot: Mutex<Slot>,
    /// Serialises start/stop; status never takes this lock.
    lifecycle: Mutex<()>,
    /// Serialises model downloads.
    download_lock: Mutex<()>,
    pub setup_running: AtomicBool,
    pub cancel: AtomicBool,
    system_runtime: Option<PathBuf>,
    pid_file: PathBuf,
}

impl Manager {
    pub fn new(paths: Paths, sink: EventSink) -> Self {
        let system_runtime = find_in_path("llama-server");
        let pid_file = paths.logs.join("llama-server.pid");
        // A hard kill (SIGKILL, crash) can leave the child running. The pid file
        // is written only by this app, so a file seen at startup is stale.
        cleanup_stale_server(&pid_file);
        Self {
            paths,
            sink,
            slot: Mutex::new(Slot::default()),
            lifecycle: Mutex::new(()),
            download_lock: Mutex::new(()),
            setup_running: AtomicBool::new(false),
            cancel: AtomicBool::new(false),
            system_runtime,
            pid_file,
        }
    }

    pub fn models_dir(&self) -> &Path {
        &self.paths.models
    }

    pub fn log_path(&self) -> PathBuf {
        self.paths.logs.join("llama-server.log")
    }

    fn emit(&self, event: Event) {
        (self.sink)(event);
    }

    fn emit_download(&self, p: DownloadProgress) {
        self.emit(Event::Download(p));
    }

    fn emit_state(&self) {
        let info = self.server_info();
        self.emit(Event::State(info));
    }

    pub fn server_info(&self) -> ServerInfo {
        let slot = self.slot.lock().unwrap();
        ServerInfo {
            state: slot.phase.as_str().to_string(),
            url: slot.url.clone(),
            managed: slot.managed,
            error: slot.error.clone(),
        }
    }

    fn runtime_executable(&self) -> Option<PathBuf> {
        if self.bundled_runtime_ready() {
            return Some(bundled_executable(&self.paths.runtime));
        }
        self.system_runtime.clone()
    }

    fn bundled_runtime_ready(&self) -> bool {
        runtime_ready_at(&self.paths.runtime)
    }

    pub fn runtime_ready(&self) -> bool {
        self.runtime_executable().is_some()
    }

    pub fn runtime_info(&self) -> RuntimeInfo {
        let bundled = self.bundled_runtime_ready();
        let system = self.system_runtime.is_some() && !bundled;
        let path = self.runtime_executable();
        RuntimeInfo {
            bundled,
            system,
            version: if bundled {
                read_version(&self.paths.runtime).or_else(|| Some(RUNTIME_VERSION.into()))
            } else {
                None
            },
            platform: format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH),
            path: path.map(|p| p.display().to_string()),
        }
    }

    pub fn model_info(&self) -> ModelInfo {
        let mut bytes = 0u64;
        let mut installed = true;
        for file in MODEL_FILES {
            let path = self.paths.models.join(file.name);
            match fs::metadata(&path) {
                Ok(meta) if meta.len() == file.bytes => bytes += file.bytes,
                _ => installed = false,
            }
        }
        let verified = installed && marker_ok(&self.paths.models);
        ModelInfo {
            installed,
            verified,
            bytes,
            total: model_total(),
            dir: self.paths.models.display().to_string(),
        }
    }

    /// External health check + managed server snapshot. Blocking (HTTP 600 ms).
    pub fn status(&self, external_url: &str) -> Status {
        let runtime = self.runtime_info();
        let model = self.model_info();
        let server = self.server_info();
        let managed_online = server.state == "online";
        let external = if managed_online {
            false
        } else {
            health_ok(external_url, Duration::from_millis(600))
        };
        let llama = managed_online || external;
        let llama_url = if managed_online {
            server.url.clone().unwrap_or_else(|| external_url.to_string())
        } else {
            external_url.to_string()
        };
        let needed = !llama && !model.installed;
        Status {
            llama,
            tesseract: false,
            llama_url,
            runtime: runtime.clone(),
            model,
            server,
            setup: SetupInfo {
                needed,
                download_bytes: model_total(),
                model_name: MODEL_NAME.to_string(),
                license: MODEL_LICENSE.to_string(),
                runtime_ready: runtime.bundled || runtime.system,
            },
        }
    }

    /// Download both weights (resuming partial files), verify them, then start the
    /// bundled server. Emits progress at every phase.
    pub fn bootstrap(&self, external_url: &str) -> Result<(), Error> {
        if !self.runtime_ready() {
            return Err(Error::RuntimeMissing(
                "此构建缺少内置 llama.cpp 运行时。".into(),
            ));
        }

        self.ensure_model()?;

        if health_ok(external_url, Duration::from_millis(600)) {
            self.emit_download(DownloadProgress {
                phase: "ready".into(),
                file: None,
                downloaded: model_total(),
                total: model_total(),
                bytes_per_sec: 0,
                eta_secs: Some(0),
                message: Some("正在使用已有的 llama.cpp 服务".into()),
            });
            return Ok(());
        }

        self.emit_download(DownloadProgress {
            phase: "starting".into(),
            file: None,
            downloaded: model_total(),
            total: model_total(),
            bytes_per_sec: 0,
            eta_secs: None,
            message: Some("正在加载本地模型".into()),
        });
        self.start_managed()?;

        self.emit_download(DownloadProgress {
            phase: "ready".into(),
            file: None,
            downloaded: model_total(),
            total: model_total(),
            bytes_per_sec: 0,
            eta_secs: Some(0),
            message: None,
        });
        Ok(())
    }

    pub fn ensure_model(&self) -> Result<ModelInfo, Error> {
        let _guard = self.download_lock.lock().unwrap();
        fs::create_dir_all(&self.paths.models).map_err(io_err)?;

        if marker_ok(&self.paths.models) && self.model_info().installed {
            return Ok(self.model_info());
        }

        if let Some(free) = fs2::available_space(&self.paths.models).ok() {
            let needed = model_total().saturating_sub(self.model_info().bytes);
            if free < needed + 256 * 1024 * 1024 {
                return Err(Error::Download(format!(
                    "磁盘空间不足：剩余 {:.1} GB，需要 {:.1} GB",
                    free as f64 / 1e9,
                    (needed + 256 * 1024 * 1024) as f64 / 1e9
                )));
            }
        }

        let total = model_total();
        let mut completed = 0u64;

        for spec in MODEL_FILES {
            if self.cancel.load(Ordering::SeqCst) {
                return Err(Error::Cancelled);
            }
            let dest = self.paths.models.join(spec.name);
            let part = self.paths.models.join(format!("{}.part", spec.name));

            if let Ok(meta) = fs::metadata(&dest) {
                if meta.len() == spec.bytes {
                    self.emit_verifying(completed + spec.bytes, total, spec.name);
                    if sha256_file(&dest)? == spec.sha256 {
                        completed += spec.bytes;
                        continue;
                    }
                }
                let _ = fs::remove_file(&dest);
            }

            if let Ok(meta) = fs::metadata(&part) {
                if meta.len() >= spec.bytes {
                    let _ = fs::remove_file(&part);
                }
            }

            self.download_file(spec, &part, completed, total)?;

            self.emit_verifying(completed + spec.bytes, total, spec.name);
            let actual = sha256_file(&part)?;
            if actual != spec.sha256 {
                let _ = fs::remove_file(&part);
                return Err(Error::Download(format!(
                    "{} SHA-256 校验失败（期望 {}，实际 {}）",
                    spec.name, spec.sha256, actual
                )));
            }
            let _ = fs::remove_file(&dest);
            fs::rename(&part, &dest).map_err(io_err)?;
            completed += spec.bytes;
        }

        write_marker(&self.paths.models)?;
        let model = self.model_info();
        if !model.installed {
            return Err(Error::Download("下载后模型文件不完整。".into()));
        }
        Ok(model)
    }

    fn emit_verifying(&self, completed: u64, total: u64, file: &str) {
        self.emit_download(DownloadProgress {
            phase: "verifying".into(),
            file: Some(file.to_string()),
            downloaded: completed,
            total,
            bytes_per_sec: 0,
            eta_secs: None,
            message: None,
        });
    }

    fn download_file(
        &self,
        spec: ModelFile,
        part: &Path,
        completed_before: u64,
        total: u64,
    ) -> Result<(), Error> {
        let url = model_url(spec.name);
        self.download_file_from(&url, spec, part, completed_before, total)
    }

    fn download_file_from(
        &self,
        url: &str,
        spec: ModelFile,
        part: &Path,
        completed_before: u64,
        total: u64,
    ) -> Result<(), Error> {
        let mut offset = fs::metadata(part).map(|m| m.len()).unwrap_or(0);
        if offset > spec.bytes {
            let _ = fs::remove_file(part);
            offset = 0;
        }

        let mut agent_builder = ureq::AgentBuilder::new()
            .timeout_connect(Duration::from_secs(20))
            .timeout_read(Duration::from_secs(60))
            .user_agent(concat!("ocr4code/", env!("CARGO_PKG_VERSION")));
        if let Some(proxy) = proxy_for_url(url) {
            agent_builder = agent_builder.proxy(proxy);
        }
        let agent = agent_builder.build();

        let mut request = agent.get(url);
        if offset > 0 {
            request = request.set("Range", &format!("bytes={offset}-"));
        }
        let response = match request.call() {
            Ok(r) => r,
            Err(ureq::Error::Status(416, _)) if offset == spec.bytes => {
                return Ok(());
            }
            Err(err) => {
                return Err(Error::Download(format!(
                    "下载 {} 失败：{err}",
                    spec.name
                )))
            }
        };

        let status = response.status();
        if offset > 0 && status == 200 {
            // Server ignored Range; start over rather than appending a whole file.
            offset = 0;
        }

        let mut file = if offset == 0 {
            File::create(part).map_err(io_err)?
        } else {
            OpenOptions::new().append(true).open(part).map_err(io_err)?
        };

        let mut downloaded = offset;
        let started = Instant::now();
        let mut last_tick = Instant::now();
        let mut last_pos = offset;
        let mut speed = 0f64;
        let mut reader = response.into_reader();

        self.emit_download(download_progress(
            spec.name,
            completed_before + downloaded,
            total,
            0,
            None,
        ));

        let mut buf = vec![0u8; 512 * 1024];
        loop {
            if self.cancel.load(Ordering::SeqCst) {
                let _ = file.flush();
                return Err(Error::Cancelled);
            }
            let n = reader.read(&mut buf).map_err(|e| {
                Error::Download(format!("读取 {} 失败：{e}", spec.name))
            })?;
            if n == 0 {
                break;
            }
            file.write_all(&buf[..n]).map_err(io_err)?;
            downloaded += n as u64;

            let now = Instant::now();
            let dt = now.duration_since(last_tick).as_secs_f64();
            if dt >= 0.25 {
                let instant = (downloaded - last_pos) as f64 / dt.max(0.001);
                speed = if speed == 0.0 {
                    instant
                } else {
                    speed * 0.65 + instant * 0.35
                };
                let remaining = total.saturating_sub(completed_before + downloaded);
                let eta = if speed > 1024.0 {
                    Some((remaining as f64 / speed) as u64)
                } else {
                    None
                };
                self.emit_download(download_progress(
                    spec.name,
                    completed_before + downloaded,
                    total,
                    speed as u64,
                    eta,
                ));
                last_tick = now;
                last_pos = downloaded;
            }
        }
        let _ = file.flush();

        if downloaded < spec.bytes {
            return Err(Error::Download(format!(
                "{} 下载不完整（{}/{} 字节，耗时 {:.0} 秒）",
                spec.name,
                downloaded,
                spec.bytes,
                started.elapsed().as_secs_f64()
            )));
        }

        self.emit_download(download_progress(
            spec.name,
            completed_before + spec.bytes,
            total,
            speed as u64,
            Some(0),
        ));
        Ok(())
    }

    /// Use an already-running external server, otherwise start the managed one.
    pub fn ensure_ready(&self, external_url: &str) -> Result<String, Error> {
        if let Some(url) = self.online_url_if_healthy() {
            return Ok(url);
        }
        if health_ok(external_url, Duration::from_millis(600)) {
            return Ok(external_url.to_string());
        }
        if !self.runtime_ready() {
            return Err(Error::RuntimeMissing(
                "本地 llama.cpp 运行时缺失；请使用内置运行时的安装包。"
                    .into(),
            ));
        }
        if !self.model_info().installed {
            return Err(Error::SetupRequired);
        }
        self.start_managed()
    }

    fn online_url_if_healthy(&self) -> Option<String> {
        let (url, online) = {
            let slot = self.slot.lock().unwrap();
            (slot.url.clone(), slot.phase == Phase::Online)
        };
        if let Some(url) = url {
            if online && health_ok(&url, Duration::from_millis(500)) {
                return Some(url);
            }
        }
        None
    }

    pub fn start_managed(&self) -> Result<String, Error> {
        let _lifecycle = self.lifecycle.lock().unwrap();

        if let Some(url) = self.online_url_if_healthy() {
            return Ok(url);
        }
        self.stop_child();

        let exe = self.runtime_executable().ok_or_else(|| {
            Error::RuntimeMissing("内置 llama.cpp 运行时缺失。".into())
        })?;
        if !self.model_info().installed {
            return Err(Error::SetupRequired);
        }

        let model = self.paths.models.join(MODEL_FILES[0].name);
        let mmproj = self.paths.models.join(MODEL_FILES[1].name);
        let port = pick_port();
        let url = format!("http://127.0.0.1:{port}");
        let log_path = self.log_path();
        if let Some(dir) = log_path.parent() {
            fs::create_dir_all(dir).map_err(io_err)?;
        }
        let log = File::create(&log_path).map_err(io_err)?;
        let log_err = log.try_clone().map_err(io_err)?;

        let mut command = Command::new(&exe);
        command
            .current_dir(exe.parent().unwrap_or(Path::new(".")))
            .arg("-m")
            .arg(&model)
            .arg("--mmproj")
            .arg(&mmproj)
            .arg("--host")
            .arg("127.0.0.1")
            .arg("--port")
            .arg(port.to_string())
            .arg("-c")
            .arg("32768")
            .arg("-np")
            .arg("3")
            .arg("-ngl")
            .arg(if cfg!(target_os = "macos") { "99" } else { "0" })
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
            Error::Server(format!(
                "无法启动 llama-server（{}）：{e}",
                exe.display()
            ))
        })?;
        write_pid(&self.pid_file, child.id());

        {
            let mut slot = self.slot.lock().unwrap();
            slot.child = Some(child);
            slot.url = Some(url.clone());
            slot.phase = Phase::Starting;
            slot.error = None;
            slot.managed = true;
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

    fn stop_child(&self) {
        let child = {
            let mut slot = self.slot.lock().unwrap();
            let child = slot.child.take();
            slot.phase = Phase::Stopped;
            slot.url = None;
            slot.error = None;
            slot.managed = false;
            child
        };
        if let Some(mut child) = child {
            let _ = child.kill();
            let _ = child.wait();
        }
        let _ = fs::remove_file(&self.pid_file);
        self.emit_state();
    }

    pub fn remove_model(&self) -> Result<(), Error> {
        self.stop();
        if self.paths.models.exists() {
            fs::remove_dir_all(&self.paths.models).map_err(io_err)?;
        }
        Ok(())
    }

    pub fn reset_cancel(&self) {
        self.cancel.store(false, Ordering::SeqCst);
    }
}

impl Drop for Manager {
    fn drop(&mut self) {
        if let Ok(mut slot) = self.slot.lock() {
            if let Some(mut child) = slot.child.take() {
                let _ = child.kill();
                let _ = child.wait();
            }
        }
    }
}

fn download_progress(
    file: &str,
    downloaded: u64,
    total: u64,
    bps: u64,
    eta: Option<u64>,
) -> DownloadProgress {
    DownloadProgress {
        phase: "downloading".into(),
        file: Some(file.to_string()),
        downloaded,
        total,
        bytes_per_sec: bps,
        eta_secs: eta,
        message: None,
    }
}

fn write_pid(path: &Path, pid: u32) {
    if let Some(dir) = path.parent() {
        let _ = fs::create_dir_all(dir);
    }
    let _ = fs::write(path, pid.to_string());
}

fn cleanup_stale_server(pid_file: &Path) {
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

fn io_err(err: std::io::Error) -> Error {
    Error::Download(err.to_string())
}

fn sha256_file(path: &Path) -> Result<String, Error> {
    let mut file = File::open(path).map_err(io_err)?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 1024 * 1024];
    loop {
        let n = file.read(&mut buf).map_err(io_err)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

#[derive(Serialize, Deserialize)]
struct Marker {
    revision: String,
    generated_at: u64,
}

fn marker_ok(dir: &Path) -> bool {
    let Ok(raw) = fs::read_to_string(dir.join("verified.json")) else {
        return false;
    };
    serde_json::from_str::<Marker>(&raw)
        .map(|m| m.revision == MODEL_REVISION)
        .unwrap_or(false)
}

fn write_marker(dir: &Path) -> Result<(), Error> {
    let marker = Marker {
        revision: MODEL_REVISION.into(),
        generated_at: SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs(),
    };
    let body = serde_json::to_vec_pretty(&marker).map_err(|e| Error::Download(e.to_string()))?;
    fs::write(dir.join("verified.json"), body).map_err(io_err)
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

/// Many developer machines reach Hugging Face through a local proxy. `ureq`
/// does not read the conventional environment variables itself, so honour them
/// for the download path only (local health checks stay direct).
fn proxy_for_url(url: &str) -> Option<ureq::Proxy> {
    let lower = url.to_lowercase();
    if lower.contains("://127.0.0.1") || lower.contains("://localhost") || lower.contains("://[::1]") {
        return None;
    }
    proxy_from_env()
}

fn proxy_from_env() -> Option<ureq::Proxy> {
    for key in [
        "HTTPS_PROXY",
        "https_proxy",
        "ALL_PROXY",
        "all_proxy",
        "HTTP_PROXY",
        "http_proxy",
    ] {
        let Ok(value) = std::env::var(key) else {
            continue;
        };
        let value = value.trim();
        if value.is_empty() {
            continue;
        }
        match ureq::Proxy::new(value) {
            Ok(proxy) => return Some(proxy),
            Err(err) => eprintln!("引擎：忽略无效的代理配置 {key}：{err}"),
        }
    }
    None
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

fn read_version(runtime_dir: &Path) -> Option<String> {
    fs::read_to_string(runtime_dir.join("VERSION"))
        .ok()
        .and_then(|s| s.lines().next().map(str::to_string))
        .filter(|s| !s.trim().is_empty())
}

fn executable_name() -> &'static str {
    if cfg!(target_os = "windows") {
        "llama-server.exe"
    } else {
        "llama-server"
    }
}

fn bundled_executable(runtime_dir: &Path) -> PathBuf {
    runtime_dir.join(executable_name())
}

#[cfg(target_os = "macos")]
const REQUIRED_LIBS: &[&str] = &[
    "libllama.0.dylib",
    "libllama-common.0.dylib",
    "libllama-server-impl.dylib",
    "libmtmd.0.dylib",
    "libggml.0.dylib",
    "libggml-base.0.dylib",
    "libggml-cpu.0.dylib",
    "libggml-metal.0.dylib",
    "libggml-blas.0.dylib",
    "libggml-rpc.0.dylib",
];

#[cfg(target_os = "windows")]
const REQUIRED_LIBS: &[&str] = &[
    "llama-server-impl.dll",
    "llama-common.dll",
    "llama.dll",
    "ggml.dll",
    "ggml-base.dll",
    "ggml-cpu-x64.dll",
    "mtmd.dll",
];

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
const REQUIRED_LIBS: &[&str] = &[];

pub fn runtime_ready_at(dir: &Path) -> bool {
    let exe = bundled_executable(dir);
    if !exe.is_file() {
        return false;
    }
    REQUIRED_LIBS.iter().all(|lib| dir.join(lib).is_file())
}

fn find_in_path(name: &str) -> Option<PathBuf> {
    let exe = if cfg!(target_os = "windows") {
        format!("{name}.exe")
    } else {
        name.to_string()
    };
    std::env::var_os("PATH").and_then(|paths| {
        std::env::split_paths(&paths).find_map(|dir| {
            let candidate = dir.join(&exe);
            candidate.is_file().then_some(candidate)
        })
    })
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manifest_sizes_match_total() {
        assert_eq!(model_total(), MODEL_TOTAL_BYTES);
        assert!(MODEL_FILES.iter().all(|f| f.bytes > 0 && f.sha256.len() == 64));
    }

    #[test]
    fn sha256_matches_a_known_value() {
        let dir = std::env::temp_dir().join(format!("ocr4code-sha-{}", std::process::id()));
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
            let status = if start > 0 { "206 Partial Content" } else { "200 OK" };
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

        let dir = std::env::temp_dir().join(format!(
            "ocr4code-resume-{}-{}",
            std::process::id(),
            port
        ));
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
    fn marker_round_trip() {
        let dir = std::env::temp_dir().join(format!("ocr4code-marker-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        assert!(!marker_ok(&dir));
        write_marker(&dir).unwrap();
        assert!(marker_ok(&dir));
        let _ = fs::remove_dir_all(&dir);
    }
}
