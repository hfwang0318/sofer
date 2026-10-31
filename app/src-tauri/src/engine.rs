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
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub const RUNTIME_VERSION: &str = "llama.cpp b12018";
pub const MODEL_LICENSE: &str = "MIT";

/// Which weights the app should load. Both packs share the same Q8 mmproj.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ModelQuality {
    High,
    Low,
}

impl Default for ModelQuality {
    fn default() -> Self {
        Self::High
    }
}

impl ModelQuality {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::High => "high",
            Self::Low => "low",
        }
    }
}

const OFFICIAL_HOST: &str = "https://huggingface.co";
const MIRROR_HOST: &str = "https://hf-mirror.com";
const OFFICIAL_REPO: &str = "ggml-org/GLM-OCR-GGUF";
const OFFICIAL_REVISION: &str = "65a42de1148dbed2297e922b5dbc7d9b70c36578";
const Q4_REPO: &str = "mradermacher/GLM-OCR-GGUF";
const Q4_REVISION: &str = "11fad2c98e06d1ca82a1b5bc0c9137ee5758991b";

#[derive(Debug, Clone, Copy)]
pub struct ModelFile {
    pub name: &'static str,
    pub bytes: u64,
    pub sha256: &'static str,
    pub repo: &'static str,
    pub revision: &'static str,
}

#[derive(Debug, Clone, Copy)]
pub struct ModelPack {
    pub quality: ModelQuality,
    pub name: &'static str,
    pub files: &'static [ModelFile],
}

const MMPROJ: ModelFile = ModelFile {
    name: "mmproj-GLM-OCR-Q8_0.gguf",
    bytes: 484_403_648,
    sha256: "9c4b58e33e316ed142eb5dcb41abec3844d3e6e5dc361ffb782c3fa9d175141f",
    repo: OFFICIAL_REPO,
    revision: OFFICIAL_REVISION,
};

static HIGH_FILES: [ModelFile; 2] = [
    ModelFile {
        name: "GLM-OCR-Q8_0.gguf",
        bytes: 950_433_408,
        sha256: "45bc244a6446aff850521dc41f18bc8d7105ad5f0c2c8c28af04e7cc4f4d50b1",
        repo: OFFICIAL_REPO,
        revision: OFFICIAL_REVISION,
    },
    MMPROJ,
];

static LOW_FILES: [ModelFile; 2] = [
    ModelFile {
        name: "GLM-OCR.Q4_K_M.gguf",
        bytes: 548_515_136,
        sha256: "bb27f2451f0f1b13491e04e0664cd946438a4677c32acf8ecfef79f68e862de1",
        repo: Q4_REPO,
        revision: Q4_REVISION,
    },
    MMPROJ,
];

static HIGH_PACK: ModelPack = ModelPack {
    quality: ModelQuality::High,
    name: "GLM-OCR Q8_0",
    files: &HIGH_FILES,
};

static LOW_PACK: ModelPack = ModelPack {
    quality: ModelQuality::Low,
    name: "GLM-OCR Q4_K_M",
    files: &LOW_FILES,
};

pub fn model_pack(quality: ModelQuality) -> &'static ModelPack {
    match quality {
        ModelQuality::High => &HIGH_PACK,
        ModelQuality::Low => &LOW_PACK,
    }
}

pub fn model_files(quality: ModelQuality) -> &'static [ModelFile] {
    model_pack(quality).files
}

pub fn model_total(quality: ModelQuality) -> u64 {
    model_files(quality).iter().map(|f| f.bytes).sum()
}

pub fn model_name(quality: ModelQuality) -> &'static str {
    model_pack(quality).name
}

/// Stable identifier of the exact file set, used by the `verified-*.json` marker.
pub fn pack_manifest(quality: ModelQuality) -> String {
    model_files(quality)
        .iter()
        .map(|f| format!("{}:{}", f.name, f.sha256))
        .collect::<Vec<_>>()
        .join(",")
}

/// Download options from the settings page: which pack, whether to use the
/// `hf-mirror.com` host, and an already-composed proxy URL.
#[derive(Debug, Clone, Default)]
pub struct DownloadOptions {
    pub quality: ModelQuality,
    pub use_mirror: bool,
    pub proxy: Option<String>,
}

pub fn model_url(file: &ModelFile, use_mirror: bool) -> String {
    let env_base = std::env::var("OCR4CODE_MODEL_BASE").ok();
    model_url_with_base(file, env_base.as_deref(), use_mirror)
}

fn model_url_with_base(
    file: &ModelFile,
    override_base: Option<&str>,
    use_mirror: bool,
) -> String {
    // Escape hatch for an internal mirror / offline staging server. The base
    // must include the resolve path, e.g.
    // https://mirror.example/ggml-org/GLM-OCR-GGUF/resolve/<revision>
    if let Some(base) = override_base
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        return format!("{}/{}", base.trim_end_matches('/'), file.name);
    }
    let host = if use_mirror { MIRROR_HOST } else { OFFICIAL_HOST };
    format!(
        "{host}/{}/resolve/{}/{}",
        file.repo, file.revision, file.name
    )
}

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
            kv_cache_quant: cfg!(target_os = "macos"),
            idle_sleep_secs: 300,
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
    /// Which weight pack the managed server was started with.
    pub quality: Option<ModelQuality>,
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
    RangeUnsupported,
    Server(String),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::SetupRequired => write!(f, "识别模型还没有下载"),
            Error::Cancelled => write!(f, "设置已取消"),
            Error::RuntimeMissing(m) => write!(f, "{m}"),
            Error::Download(m) => write!(f, "{m}"),
            Error::RangeUnsupported => write!(f, "服务器不支持分片下载"),
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
    config: Option<ServerConfig>,
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
            quality: slot.config.map(|config| config.quality),
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

    pub fn model_info(&self, quality: ModelQuality) -> ModelInfo {
        let mut bytes = 0u64;
        let mut installed = true;
        for file in model_files(quality) {
            let path = self.paths.models.join(file.name);
            match fs::metadata(&path) {
                Ok(meta) if meta.len() == file.bytes => bytes += file.bytes,
                _ => installed = false,
            }
        }
        let verified = installed && marker_ok(&self.paths.models, quality);
        ModelInfo {
            installed,
            verified,
            bytes,
            total: model_total(quality),
            dir: self.paths.models.display().to_string(),
        }
    }

    /// External health check + managed server snapshot. Blocking (HTTP 600 ms).
    pub fn status(&self, external_url: &str, quality: ModelQuality) -> Status {
        let runtime = self.runtime_info();
        let model = self.model_info(quality);
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
        // A running managed server with the other weight pack is not ready for
        // the selected quality: prompt for the missing download or a restart.
        let needed = if managed_online {
            server.quality != Some(quality) || !model.installed
        } else if external {
            false
        } else {
            !model.installed
        };
        Status {
            llama,
            tesseract: false,
            llama_url,
            runtime: runtime.clone(),
            model,
            server,
            setup: SetupInfo {
                needed,
                download_bytes: model_total(quality),
                model_name: model_name(quality).to_string(),
                license: MODEL_LICENSE.to_string(),
                runtime_ready: runtime.bundled || runtime.system,
            },
        }
    }

    /// Download both weights (resuming partial files), verify them, then start the
    /// bundled server. Emits progress at every phase.
    pub fn bootstrap(
        &self,
        external_url: &str,
        options: &DownloadOptions,
        server: ServerConfig,
    ) -> Result<(), Error> {
        if !self.runtime_ready() {
            return Err(Error::RuntimeMissing(
                "这个安装包缺少内置识别运行时。请重新安装完整版本。".into(),
            ));
        }

        self.ensure_model(options)?;

        let total = model_total(options.quality);
        if health_ok(external_url, Duration::from_millis(600)) {
            self.emit_download(DownloadProgress {
                phase: "ready".into(),
                file: None,
                downloaded: total,
                total,
                bytes_per_sec: 0,
                eta_secs: Some(0),
                message: Some("正在使用已有的 llama.cpp 服务".into()),
            });
            return Ok(());
        }

        self.emit_download(DownloadProgress {
            phase: "starting".into(),
            file: None,
            downloaded: total,
            total,
            bytes_per_sec: 0,
            eta_secs: None,
            message: Some("正在加载本地模型".into()),
        });
        self.start_managed(server)?;

        self.emit_download(DownloadProgress {
            phase: "ready".into(),
            file: None,
            downloaded: total,
            total,
            bytes_per_sec: 0,
            eta_secs: Some(0),
            message: None,
        });
        Ok(())
    }

    pub fn ensure_model(&self, options: &DownloadOptions) -> Result<ModelInfo, Error> {
        let _guard = self.download_lock.lock().unwrap();
        fs::create_dir_all(&self.paths.models).map_err(io_err)?;

        let quality = options.quality;
        let total = model_total(quality);
        if marker_ok(&self.paths.models, quality) && self.model_info(quality).installed {
            return Ok(self.model_info(quality));
        }

        let installed = self.model_info(quality);
        if let Some(free) = fs2::available_space(&self.paths.models).ok() {
            let needed = total.saturating_sub(installed.bytes);
            if free < needed + 256 * 1024 * 1024 {
                return Err(Error::Download(format!(
                    "磁盘空间不足：剩余 {:.1} GB，需要 {:.1} GB",
                    free as f64 / 1e9,
                    (needed + 256 * 1024 * 1024) as f64 / 1e9
                )));
            }
        }

        let mut completed = 0u64;

        for &spec in model_files(quality) {
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

            self.download_file(spec, &part, completed, total, options)?;

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

        write_marker(&self.paths.models, quality)?;
        let model = self.model_info(quality);
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
        options: &DownloadOptions,
    ) -> Result<(), Error> {
        let url = model_url(&spec, options.use_mirror);
        let agent = build_agent(&url, options.proxy.as_deref())?;

        if spec.bytes >= PARALLEL_MIN_BYTES && ranges_supported(&agent, &url)? {
            match self.download_parallel(
                &agent,
                &url,
                spec,
                part,
                completed_before,
                total,
            )? {
                ParallelOutcome::Done => return Ok(()),
                ParallelOutcome::RangeUnsupported => {
                    // Probe said 206 but a worker got 200. `download_parallel`
                    // already kept chunk 0 as the contiguous prefix; finish with
                    // the single-stream path.
                }
            }
        }

        self.download_file_single_with(
            &agent,
            &url,
            spec,
            part,
            completed_before,
            total,
        )
    }

    /// Test/helper entry point without a settings override.
    #[cfg(test)]
    fn download_file_from(
        &self,
        url: &str,
        spec: ModelFile,
        part: &Path,
        completed_before: u64,
        total: u64,
    ) -> Result<(), Error> {
        self.download_file_from_with(url, spec, part, completed_before, total, None)
    }

    #[cfg(test)]
    fn download_file_from_with(
        &self,
        url: &str,
        spec: ModelFile,
        part: &Path,
        completed_before: u64,
        total: u64,
        proxy_override: Option<&str>,
    ) -> Result<(), Error> {
        let agent = build_agent(url, proxy_override)?;
        self.download_file_single_with(&agent, url, spec, part, completed_before, total)
    }

    fn download_parallel(
        &self,
        agent: &ureq::Agent,
        url: &str,
        spec: ModelFile,
        part: &Path,
        completed_before: u64,
        total: u64,
    ) -> Result<ParallelOutcome, Error> {
        let mut legacy = fs::metadata(part).map(|m| m.len()).unwrap_or(0);
        if legacy > spec.bytes {
            let _ = fs::remove_file(part);
            legacy = 0;
        }
        if legacy == spec.bytes {
            return Ok(ParallelOutcome::Done);
        }

        let chunk0 = chunk_path(part, 0);
        let chunks = match read_chunk_plan(part, spec.bytes) {
            Some(chunks) => chunks,
            None => {
                // A legacy sequential `.part` is a contiguous prefix. Keep it as
                // chunk 0 and split the remainder across the other connections.
                let prefix = if legacy > 0 {
                    legacy
                } else {
                    fs::metadata(&chunk0).map(|m| m.len()).unwrap_or(0)
                };
                let chunks = build_chunk_plan(spec.bytes, prefix);
                if chunks.len() <= 1 {
                    return Ok(ParallelOutcome::RangeUnsupported);
                }
                write_chunk_plan(part, spec.bytes, &chunks)?;
                chunks
            }
        };
        if chunks.len() <= 1 {
            return Ok(ParallelOutcome::RangeUnsupported);
        }

        if legacy > 0 && !chunk0.exists() {
            fs::rename(part, &chunk0).map_err(io_err)?;
        }

        let mut initial = 0u64;
        for (index, chunk) in chunks.iter().enumerate() {
            let expected = chunk.end - chunk.start;
            let path = chunk_path(part, index);
            let mut len = fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
            if len > expected {
                let _ = fs::remove_file(&path);
                len = 0;
            }
            initial += len;
        }

        if initial == spec.bytes {
            join_chunks(part, &chunks, spec.bytes)?;
            return Ok(ParallelOutcome::Done);
        }

        let downloaded = std::sync::atomic::AtomicU64::new(initial);
        let done = AtomicBool::new(false);
        let abort = AtomicBool::new(false);
        self.emit_download(download_progress(
            spec.name,
            completed_before + initial,
            total,
            0,
            None,
        ));

        let outcome = std::thread::scope(|scope| {
            let reporter = scope.spawn(|| {
                let mut last = Instant::now();
                let mut last_pos = initial;
                let mut speed = 0f64;
                while !done.load(Ordering::Relaxed) {
                    std::thread::sleep(Duration::from_millis(250));
                    let current = downloaded.load(Ordering::Relaxed);
                    let now = Instant::now();
                    let dt = now.duration_since(last).as_secs_f64().max(0.001);
                    let instant = current.saturating_sub(last_pos) as f64 / dt;
                    speed = if speed == 0.0 {
                        instant
                    } else {
                        speed * 0.65 + instant * 0.35
                    };
                    let remaining = total.saturating_sub(completed_before + current);
                    let eta = if speed > 1024.0 {
                        Some((remaining as f64 / speed) as u64)
                    } else {
                        None
                    };
                    self.emit_download(download_progress(
                        spec.name,
                        completed_before + current,
                        total,
                        speed as u64,
                        eta,
                    ));
                    last = now;
                    last_pos = current;
                }
            });

            let mut handles = Vec::with_capacity(chunks.len());
            for index in 0..chunks.len() {
                let chunk = chunks[index];
                let path = chunk_path(part, index);
                let name = spec.name;
                let worker_agent = agent.clone();
                let downloaded = &downloaded;
                let abort = &abort;
                let cancel = &self.cancel;
                handles.push(scope.spawn(move || {
                    download_chunk(
                        &worker_agent,
                        url,
                        name,
                        chunk,
                        &path,
                        downloaded,
                        abort,
                        cancel,
                    )
                }));
            }

            let mut first_error: Option<Error> = None;
            let mut unsupported = false;
            for handle in handles {
                match handle.join() {
                    Ok(Ok(())) => {}
                    Ok(Err(Error::RangeUnsupported)) => {
                        unsupported = true;
                        abort.store(true, Ordering::Relaxed);
                    }
                    Ok(Err(Error::Cancelled)) => {
                        abort.store(true, Ordering::Relaxed);
                        if first_error.is_none() {
                            first_error = Some(Error::Cancelled);
                        }
                    }
                    Ok(Err(err)) => {
                        abort.store(true, Ordering::Relaxed);
                        if first_error.is_none()
                            || matches!(first_error, Some(Error::Cancelled))
                        {
                            first_error = Some(err);
                        }
                    }
                    Err(_) => {
                        abort.store(true, Ordering::Relaxed);
                        if first_error.is_none() {
                            first_error = Some(Error::Download(format!(
                                "{} 下载线程异常退出",
                                spec.name
                            )));
                        }
                    }
                }
            }
            done.store(true, Ordering::Relaxed);
            let _ = reporter.join();

            if self.cancel.load(Ordering::SeqCst) {
                return Err(Error::Cancelled);
            }
            if unsupported {
                return Ok(ParallelOutcome::RangeUnsupported);
            }
            if let Some(err) = first_error {
                return Err(err);
            }
            Ok(ParallelOutcome::Done)
        })?;

        if let ParallelOutcome::Done = outcome {
            join_chunks(part, &chunks, spec.bytes)?;
            self.emit_download(download_progress(
                spec.name,
                completed_before + spec.bytes,
                total,
                0,
                Some(0),
            ));
        } else {
            cleanup_parallel_files(part);
        }
        Ok(outcome)
    }

    fn download_file_single_with(
        &self,
        agent: &ureq::Agent,
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
    pub fn ensure_ready(
        &self,
        external_url: &str,
        config: ServerConfig,
    ) -> Result<String, Error> {
        if let Some(url) = self.online_url_if_healthy(config) {
            return Ok(url);
        }
        if health_ok(external_url, Duration::from_millis(600)) {
            return Ok(external_url.to_string());
        }
        if !self.runtime_ready() {
            return Err(Error::RuntimeMissing(
                "缺少内置识别运行时；请使用完整安装包。"
                    .into(),
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
            if online && current == Some(config) && health_ok(&url, Duration::from_millis(500))
            {
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
            Error::RuntimeMissing("缺少内置识别运行时。请重新安装完整版本。".into())
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

    pub fn remove_model(&self, quality: ModelQuality) -> Result<(), Error> {
        self.cancel.store(true, Ordering::SeqCst);
        self.stop();
        let dir = &self.paths.models;
        if !dir.exists() {
            return Ok(());
        }
        let other = match quality {
            ModelQuality::High => ModelQuality::Low,
            ModelQuality::Low => ModelQuality::High,
        };
        let other_main = model_files(other)[0].name;
        let keep_mmproj = dir.join(other_main).exists();
        for file in model_files(quality) {
            if file.name == MMPROJ.name && keep_mmproj {
                continue;
            }
            let _ = fs::remove_file(dir.join(file.name));
            let _ = fs::remove_file(dir.join(format!("{}.part", file.name)));
        }
        let _ = fs::remove_file(marker_path(dir, quality));
        let _ = fs::remove_file(dir.join("verified.json"));
        if !keep_mmproj {
            let _ = fs::remove_file(dir.join(MMPROJ.name));
            let _ = fs::remove_file(dir.join(format!("{}.part", MMPROJ.name)));
            let _ = fs::remove_file(marker_path(dir, other));
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

const PARALLEL_MIN_BYTES: u64 = 32 * 1024 * 1024;
const PARALLEL_CONNECTIONS: usize = 8;
const CHUNK_RETRIES: u32 = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ParallelOutcome {
    Done,
    RangeUnsupported,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
struct Chunk {
    start: u64,
    end: u64,
}

#[derive(Debug, Serialize, Deserialize)]
struct ChunkPlan {
    total: u64,
    chunks: Vec<Chunk>,
}

fn sibling_path(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.as_os_str().to_os_string();
    name.push(suffix);
    PathBuf::from(name)
}

fn chunk_path(part: &Path, index: usize) -> PathBuf {
    sibling_path(part, &format!(".{index}"))
}

fn plan_path(part: &Path) -> PathBuf {
    sibling_path(part, ".plan")
}

fn build_chunk_plan(total: u64, legacy: u64) -> Vec<Chunk> {
    if legacy > 0 && legacy < total {
        let mut chunks = vec![Chunk {
            start: 0,
            end: legacy,
        }];
        let remaining = total - legacy;
        let size = remaining.div_ceil((PARALLEL_CONNECTIONS - 1) as u64);
        let mut start = legacy;
        while start < total {
            let end = (start + size).min(total);
            chunks.push(Chunk { start, end });
            start = end;
        }
        return chunks;
    }
    let size = total.div_ceil(PARALLEL_CONNECTIONS as u64).max(1);
    let mut chunks = Vec::new();
    let mut start = 0;
    while start < total {
        let end = (start + size).min(total);
        chunks.push(Chunk { start, end });
        start = end;
    }
    chunks
}

fn read_chunk_plan(part: &Path, total: u64) -> Option<Vec<Chunk>> {
    let raw = fs::read_to_string(plan_path(part)).ok()?;
    let plan: ChunkPlan = serde_json::from_str(&raw).ok()?;
    if plan.total != total || plan.chunks.is_empty() || plan.chunks.len() > PARALLEL_CONNECTIONS {
        return None;
    }
    let mut cursor = 0;
    for chunk in &plan.chunks {
        if chunk.start != cursor || chunk.end <= chunk.start || chunk.end > total {
            return None;
        }
        cursor = chunk.end;
    }
    (cursor == total).then_some(plan.chunks)
}

fn write_chunk_plan(part: &Path, total: u64, chunks: &[Chunk]) -> Result<(), Error> {
    let plan = ChunkPlan {
        total,
        chunks: chunks.to_vec(),
    };
    let body = serde_json::to_vec_pretty(&plan)
        .map_err(|e| Error::Download(format!("写入分片计划失败：{e}")))?;
    fs::write(plan_path(part), body).map_err(io_err)
}

fn remove_chunk_plan(part: &Path) {
    let _ = fs::remove_file(plan_path(part));
}

fn build_agent(url: &str, proxy_override: Option<&str>) -> Result<ureq::Agent, Error> {
    let tls = ureq::native_tls::TlsConnector::new()
        .map_err(|e| Error::Download(format!("TLS 初始化失败：{e}")))?;
    let mut builder = ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(20))
        .timeout_read(Duration::from_secs(60))
        .tls_connector(std::sync::Arc::new(tls))
        .user_agent(concat!("ocr4code/", env!("CARGO_PKG_VERSION")));
    if let Some(proxy) = proxy_for_url(url, proxy_override) {
        builder = builder.proxy(proxy);
    }
    Ok(builder.build())
}

fn ranges_supported(agent: &ureq::Agent, url: &str) -> Result<bool, Error> {
    match agent
        .get(url)
        .set("Range", "bytes=0-0")
        .call()
    {
        Ok(response) => {
            let ok = response.status() == 206;
            if ok {
                // Drain the one-byte body so the pooled connection is clean.
                let mut byte = [0u8; 1];
                let _ = response.into_reader().read(&mut byte);
            }
            Ok(ok)
        }
        Err(ureq::Error::Status(416, _)) => Ok(false),
        Err(err) => Err(Error::Download(format!(
            "检查分片下载支持失败：{err}"
        ))),
    }
}

fn download_chunk_once(
    agent: &ureq::Agent,
    url: &str,
    name: &str,
    chunk: Chunk,
    path: &Path,
    downloaded: &AtomicU64,
    abort: &AtomicBool,
    cancel: &AtomicBool,
) -> Result<(), Error> {
    let expected = chunk.end - chunk.start;
    let mut offset = fs::metadata(path).map(|m| m.len()).unwrap_or(0);
    if offset > expected {
        let _ = fs::remove_file(path);
        offset = 0;
    }
    if offset == expected {
        return Ok(());
    }

    let mut file = if offset == 0 {
        File::create(path).map_err(io_err)?
    } else {
        OpenOptions::new().append(true).open(path).map_err(io_err)?
    };

    let response = agent
        .get(url)
        .set(
            "Range",
            &format!("bytes={}-{}", chunk.start + offset, chunk.end - 1),
        )
        .call()
        .map_err(|err| Error::Download(format!("下载 {name} 分片失败：{err}")))?;

    match response.status() {
        206 => {}
        200 => return Err(Error::RangeUnsupported),
        status if status == 416 && offset == expected => return Ok(()),
        status => {
            return Err(Error::Download(format!(
                "下载 {name} 分片返回 HTTP {status}"
            )))
        }
    }

    let mut reader = response.into_reader();
    let mut buf = vec![0u8; 512 * 1024];
    let mut current = offset;
    loop {
        if cancel.load(Ordering::SeqCst) || abort.load(Ordering::Relaxed) {
            let _ = file.flush();
            return Err(Error::Cancelled);
        }
        let remaining = (expected - current) as usize;
        if remaining == 0 {
            break;
        }
        let cap = remaining.min(buf.len());
        let n = reader
            .read(&mut buf[..cap])
            .map_err(|e| Error::Download(format!("读取 {name} 分片失败：{e}")))?;
        if n == 0 {
            break;
        }
        file.write_all(&buf[..n]).map_err(io_err)?;
        current += n as u64;
        downloaded.fetch_add(n as u64, Ordering::Relaxed);
    }
    let _ = file.flush();

    if current < expected {
        return Err(Error::Download(format!(
            "下载 {name} 分片不完整（{current}/{expected} 字节）"
        )));
    }
    Ok(())
}

fn download_chunk(
    agent: &ureq::Agent,
    url: &str,
    name: &str,
    chunk: Chunk,
    path: &Path,
    downloaded: &AtomicU64,
    abort: &AtomicBool,
    cancel: &AtomicBool,
) -> Result<(), Error> {
    let mut last_error = None;
    for attempt in 0..=CHUNK_RETRIES {
        if cancel.load(Ordering::SeqCst) || abort.load(Ordering::Relaxed) {
            return Err(Error::Cancelled);
        }
        match download_chunk_once(agent, url, name, chunk, path, downloaded, abort, cancel) {
            Ok(()) => return Ok(()),
            Err(Error::RangeUnsupported) => return Err(Error::RangeUnsupported),
            Err(Error::Cancelled) => return Err(Error::Cancelled),
            Err(err) => {
                last_error = Some(err);
                if attempt < CHUNK_RETRIES {
                    std::thread::sleep(Duration::from_millis(300 * (attempt as u64 + 1)));
                }
            }
        }
    }
    Err(last_error.unwrap_or_else(|| {
        Error::Download(format!("下载 {name} 分片失败"))
    }))
}

fn join_chunks(part: &Path, chunks: &[Chunk], total: u64) -> Result<(), Error> {
    let joined = sibling_path(part, ".join");
    let _ = fs::remove_file(&joined);
    {
        let mut out = File::create(&joined).map_err(io_err)?;
        for index in 0..chunks.len() {
            let path = chunk_path(part, index);
            let mut input = File::open(&path).map_err(io_err)?;
            std::io::copy(&mut input, &mut out).map_err(io_err)?;
        }
        out.flush().map_err(io_err)?;
    }
    if fs::metadata(&joined).map(|m| m.len()).unwrap_or(0) != total {
        let _ = fs::remove_file(&joined);
        return Err(Error::Download("并行下载拼接后大小不一致".into()));
    }
    let _ = fs::remove_file(part);
    fs::rename(&joined, part).map_err(io_err)?;
    for index in 0..chunks.len() {
        let _ = fs::remove_file(chunk_path(part, index));
    }
    remove_chunk_plan(part);
    Ok(())
}

fn cleanup_parallel_files(part: &Path) {
    remove_chunk_plan(part);
    let chunk0 = chunk_path(part, 0);
    if !part.exists() && chunk0.exists() {
        // Keep the contiguous prefix so the single-stream fallback can resume.
        let _ = fs::rename(&chunk0, part);
    } else {
        let _ = fs::remove_file(&chunk0);
    }
    for index in 1..=PARALLEL_CONNECTIONS {
        let _ = fs::remove_file(chunk_path(part, index));
    }
    let _ = fs::remove_file(sibling_path(part, ".join"));
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
    manifest: String,
    generated_at: u64,
}

fn marker_path(dir: &Path, quality: ModelQuality) -> PathBuf {
    dir.join(format!("verified-{}.json", quality.as_str()))
}

fn marker_ok(dir: &Path, quality: ModelQuality) -> bool {
    if let Ok(raw) = fs::read_to_string(marker_path(dir, quality)) {
        if serde_json::from_str::<Marker>(&raw)
            .map(|m| m.manifest == pack_manifest(quality))
            .unwrap_or(false)
        {
            return true;
        }
    }
    // Migration from the single-pack era: `verified.json` only ever described
    // the official Q8 pack. Accept it once so existing installs do not re-hash
    // 1.4 GB just to flip a badge.
    if quality == ModelQuality::High {
        if let Ok(raw) = fs::read_to_string(dir.join("verified.json")) {
            if let Ok(value) = serde_json::from_str::<serde_json::Value>(&raw) {
                return value.get("revision").and_then(|v| v.as_str())
                    == Some(OFFICIAL_REVISION);
            }
        }
    }
    false
}

fn write_marker(dir: &Path, quality: ModelQuality) -> Result<(), Error> {
    let marker = Marker {
        manifest: pack_manifest(quality),
        generated_at: SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs(),
    };
    let body = serde_json::to_vec_pretty(&marker).map_err(|e| Error::Download(e.to_string()))?;
    fs::write(marker_path(dir, quality), body).map_err(io_err)
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

/// Build a proxy URL from the three settings fields. Usernames/passwords are
/// percent-encoded as URL userinfo, so `p@ss:word` becomes `p%40ss%3Aword`
/// before it is appended to the address.
pub fn compose_proxy_url(
    address: &str,
    username: &str,
    password: &str,
) -> Result<Option<String>, String> {
    let address = address.trim();
    if address.is_empty() {
        if username.is_empty() && password.is_empty() {
            return Ok(None);
        }
        return Err("填写了代理认证信息，但代理地址为空。".into());
    }
    if username.is_empty() && !password.is_empty() {
        return Err("填写代理密码前，请先填写用户名。".into());
    }
    if username.chars().any(char::is_control) || password.chars().any(char::is_control) {
        return Err("代理用户名/密码不能包含换行或其他控制字符。".into());
    }

    let with_scheme = if address.contains("://") {
        address.to_string()
    } else {
        format!("http://{address}")
    };
    let Some((scheme, rest)) = with_scheme.split_once("://") else {
        return Err("代理地址格式无效。".into());
    };
    if !scheme.eq_ignore_ascii_case("http") {
        return Err("下载代理目前只支持 http:// 地址。".into());
    }
    if rest.contains('@') {
        return Err("代理地址里不要再写用户名/密码，请填到下面的认证字段。".into());
    }
    let host = rest.trim_end_matches('/');
    if host.is_empty()
        || host.contains('/')
        || host.contains('?')
        || host.contains('#')
        || host.chars().any(char::is_whitespace)
    {
        return Err("代理地址只填 host:port，例如 127.0.0.1:7890。".into());
    }
    if username.is_empty() && password.is_empty() {
        return Ok(Some(format!("http://{host}")));
    }
    let userinfo = format!(
        "{}:{}",
        percent_encode_userinfo(username),
        percent_encode_userinfo(password)
    );
    Ok(Some(format!("http://{userinfo}@{host}")))
}

/// Split a legacy full proxy URL (`http://user:pass@host:port`) into its parts.
/// Used only to migrate pre-existing settings files.
pub fn split_proxy_url(raw: &str) -> Option<(String, String, String)> {
    let value = raw.trim();
    let with_scheme = if value.contains("://") {
        value.to_string()
    } else {
        format!("http://{value}")
    };
    let (scheme, rest) = with_scheme.split_once("://")?;
    let (userinfo, host) = rest.rsplit_once('@')?;
    let (username, password) = userinfo.split_once(':').unwrap_or((userinfo, ""));
    if host.trim().is_empty() {
        return None;
    }
    Some((
        format!("{scheme}://{}", host.trim_end_matches('/')),
        percent_decode(username),
        percent_decode(password),
    ))
}

fn percent_encode_userinfo(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.as_bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                out.push(*byte as char)
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

/// Many developer machines reach Hugging Face through a local proxy. `ureq`
/// does not read the conventional environment variables itself, so honour them
/// for the download path only (local health checks stay direct).
/// `ureq` does not percent-decode proxy credentials. Users (and URL standards)
/// usually encode `@` as `%40`, so decode the userinfo before handing the URL to
/// `ureq`; its parser deliberately accepts a raw `@` in the password.
pub(crate) fn normalize_proxy_url(raw: &str) -> String {
    let value = raw.trim();
    let Some((scheme, rest)) = value.split_once("://") else {
        return value.to_string();
    };
    let Some(at) = rest.rfind('@') else {
        return value.to_string();
    };
    let (userinfo, host) = rest.split_at(at);
    format!("{scheme}://{}@{}", percent_decode(userinfo), &host[1..])
}

fn percent_decode(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let (Some(high), Some(low)) = (hex_value(bytes[i + 1]), hex_value(bytes[i + 2])) {
                out.push(high * 16 + low);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

fn proxy_for_url(url: &str, override_proxy: Option<&str>) -> Option<ureq::Proxy> {
    let lower = url.to_lowercase();
    if lower.contains("://127.0.0.1") || lower.contains("://localhost") || lower.contains("://[::1]") {
        return None;
    }
    if let Some(value) = override_proxy
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        match ureq::Proxy::new(normalize_proxy_url(value)) {
            Ok(proxy) => return Some(proxy),
            Err(err) => eprintln!("引擎：忽略无效的下载代理：{err}"),
        }
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
        match ureq::Proxy::new(normalize_proxy_url(value)) {
            Ok(proxy) => return Some(proxy),
            Err(err) => eprintln!("引擎：忽略无效的代理配置 {key}：{err}"),
        }
    }
    None
}

fn server_args(
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

    fn arg_value<'a>(args: &'a [String], key: &str) -> Option<&'a str> {
        args.windows(2)
            .find(|pair| pair[0] == key)
            .map(|pair| pair[1].as_str())
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
            assert_eq!(model_total(quality), files.iter().map(|f| f.bytes).sum::<u64>());
            assert!(model_total(quality) > 0);
            assert!(files.iter().all(|f| f.bytes > 0 && f.sha256.len() == 64));
            assert!(files.iter().all(|f| !f.repo.is_empty() && !f.revision.is_empty()));
        }
        assert!(model_total(ModelQuality::High) > model_total(ModelQuality::Low));
    }

    #[test]
    fn proxy_credentials_are_percent_encoded_for_the_url() {
        let url = compose_proxy_url(
            "127.0.0.1:7890",
            "user@name",
            "p@ss:word/?#%",
        )
        .unwrap()
        .unwrap();
        assert_eq!(
            url,
            "http://user%40name:p%40ss%3Aword%2F%3F%23%25@127.0.0.1:7890"
        );
        let normalized = normalize_proxy_url(&url);
        assert_eq!(
            normalized,
            "http://user@name:p@ss:word/?#%@127.0.0.1:7890"
        );
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
        let dir = std::env::temp_dir().join(format!(
            "ocr4code-remove-{}",
            std::process::id()
        ));
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
        let dir = std::env::temp_dir().join(format!(
            "ocr4code-legacy-marker-{}",
            std::process::id()
        ));
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

        let body: Arc<Vec<u8>> = Arc::new(
            (0..1024 * 1024)
                .map(|index| (index % 251) as u8)
                .collect(),
        );
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

        let dir = std::env::temp_dir().join(format!(
            "ocr4code-parallel-{}",
            std::process::id()
        ));
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

        let body: Arc<Vec<u8>> = Arc::new(
            (0..1024 * 1024)
                .map(|index| (index % 239) as u8)
                .collect(),
        );
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

        let dir = std::env::temp_dir().join(format!(
            "ocr4code-parallel-prefix-{}",
            std::process::id()
        ));
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
        let dir = std::env::temp_dir().join(format!("ocr4code-marker-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        assert!(!marker_ok(&dir, ModelQuality::High));
        write_marker(&dir, ModelQuality::High).unwrap();
        assert!(marker_ok(&dir, ModelQuality::High));
        assert!(!marker_ok(&dir, ModelQuality::Low));
        let _ = fs::remove_dir_all(&dir);
    }
}
