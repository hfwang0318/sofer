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
//!
//! The work is split by concern: [`manifest`] holds the pinned packs and URLs,
//! [`download`] the resumable transfers, [`runtime`] the bundled binary,
//! [`server`] the process lifecycle and [`proxy`] the download proxy.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Child;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::Serialize;

mod download;
mod manifest;
mod proxy;
mod runtime;
mod server;

#[cfg(test)]
mod tests;

pub use self::manifest::{
    model_files, model_name, model_pack, model_total, model_url, pack_manifest, DownloadOptions,
    ModelFile, ModelPack, ModelQuality, MODEL_LICENSE,
};
pub(crate) use self::proxy::normalize_proxy_url;
pub use self::proxy::{compose_proxy_url, split_proxy_url};
pub use self::runtime::{runtime_ready_at, RUNTIME_VERSION};
pub use self::server::{
    health_ok, llama_slots, per_slot_ctx, total_ctx, warm_up, ServerConfig, CONTEXT_MARGIN,
    IMMEDIATE_RELEASE_SECS, PROMPT_TOKEN_BUDGET,
};

use self::manifest::{marker_ok, marker_path, MMPROJ};
use self::runtime::find_in_path;
use self::server::cleanup_stale_server;

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
            Error::SetupRequired => write!(f, "OCR 模型还没有下载"),
            Error::Cancelled => write!(f, "设置已取消"),
            Error::RuntimeMissing(m) => write!(f, "{m}"),
            Error::Download(m) => write!(f, "{m}"),
            Error::RangeUnsupported => write!(f, "服务器不支持分片下载"),
            Error::Server(m) => write!(f, "{m}"),
        }
    }
}

impl std::error::Error for Error {}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum Phase {
    #[default]
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
}

impl Manager {
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
            server
                .url
                .clone()
                .unwrap_or_else(|| external_url.to_string())
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
                "这个安装包缺少内置 OCR 运行时。请重新安装完整版本。".into(),
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
}

impl Manager {
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

/// Map an I/O failure onto the error the setup UI already renders.
fn io_err(err: std::io::Error) -> Error {
    Error::Download(err.to_string())
}
