//! Deploy the OCR model to a Linux server over SSH and tunnel it back.
//!
//! The desktop app prefers the managed local server. When the user enables a
//! remote server, this module owns the whole lifecycle instead:
//!
//! * probe the target (OS, arch, tools, existing model, free disk)
//! * install the pinned llama.cpp runtime on the server, or reuse one already there
//! * download the weights on the server, or stream the local copies over SSH
//! * verify SHA-256 and write a marker next to the weights
//! * start `llama-server` bound to `127.0.0.1` and forward it with `ssh -L`
//!
//! Authentication is key / ssh-agent only (`BatchMode=yes`): the app never
//! stores a password and never prompts on a hidden terminal. The reverse side
//! of the tunnel stays on the loopback interface, so the model is not exposed
//! on the network.

use std::fs::{self, File};
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::engine::{
    health_ok, llama_slots, model_files, model_url, pack_manifest, total_ctx, ModelQuality,
};

const RUNTIME_TAG: &str = "b12018";
const RUNTIME_BASE: &str = "https://github.com/ggml-org/llama.cpp/releases/download";
const LOCAL_PORT_START: u16 = 28099;
const LOCAL_PORT_END: u16 = 28_120;
const STARTUP_TIMEOUT: Duration = Duration::from_secs(240);
const SSH_TIMEOUT: Duration = Duration::from_secs(30);
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(4 * 60 * 60);

/// Where the weights come from when the server does not have them yet.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Source {
    /// Use whatever is already there; otherwise download on the server.
    Auto,
    /// Always download on the server.
    Download,
    /// Stream the verified local copy over SSH.
    Upload,
}

impl Default for Source {
    fn default() -> Self {
        Self::Auto
    }
}

/// Which llama.cpp Linux build to install when the server has none.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Accel {
    Auto,
    Cpu,
    Vulkan,
    Cuda,
}

impl Default for Accel {
    fn default() -> Self {
        Self::Auto
    }
}

#[derive(Debug, Clone)]
pub struct RemoteConfig {
    pub host: String,
    pub user: String,
    pub ssh_port: u16,
    pub key_path: String,
    /// May start with `~`; the probe expands it before any command uses it.
    pub dir: String,
    pub server_port: u16,
    /// 0 asks the app to pick a free local port for the tunnel.
    pub local_port: u16,
    pub accel: Accel,
    pub gpu_layers: u32,
    pub quality: ModelQuality,
    pub source: Source,
    pub jobs: u32,
    pub max_tokens: u32,
    pub kv_cache_quant: bool,
    pub idle_sleep_secs: u64,
    pub use_mirror: bool,
}

#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ProbeFile {
    pub name: String,
    pub bytes: u64,
    pub expected: u64,
    pub present: bool,
}

#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Probe {
    pub reachable: bool,
    pub os: String,
    pub arch: String,
    pub home: String,
    /// Absolute remote directory after `~` expansion.
    pub dir: String,
    pub runtime: Option<String>,
    pub model_installed: bool,
    pub model_verified: bool,
    pub files: Vec<ProbeFile>,
    pub disk_free: u64,
    pub curl: bool,
    pub wget: bool,
    pub tar: bool,
    /// `sha256sum` or `shasum`; empty when neither exists.
    pub sha256: String,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Progress {
    /// connecting | probing | runtime | downloading | uploading | verifying | starting
    pub phase: String,
    pub message: Option<String>,
    pub file: Option<String>,
    pub downloaded: u64,
    pub total: u64,
    pub bytes_per_sec: u64,
    pub eta_secs: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct State {
    pub enabled: bool,
    /// stopped | probing | deploying | starting | online | failed | cancelled
    pub phase: String,
    pub url: Option<String>,
    pub remote_url: Option<String>,
    pub tunnel: bool,
    pub error: Option<String>,
    pub quality: Option<ModelQuality>,
    pub deployed: bool,
    pub progress: Option<Progress>,
}

#[derive(Debug)]
pub enum Error {
    Config(String),
    Ssh(String),
    Remote(String),
    NotDeployed(String),
    /// A file exists with the right size but the wrong SHA-256.
    Corrupt(String),
    Timeout(String),
    Cancelled,
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Config(m)
            | Error::Ssh(m)
            | Error::Remote(m)
            | Error::NotDeployed(m)
            | Error::Corrupt(m)
            | Error::Timeout(m) => write!(f, "{m}"),
            Error::Cancelled => write!(f, "远程部署已取消"),
        }
    }
}

impl std::error::Error for Error {}

#[derive(Debug, Clone)]
pub enum Event {
    Progress(Progress),
    State(State),
}

pub type EventSink = Arc<dyn Fn(Event) + Send + Sync>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    Stopped,
    Probing,
    Deploying,
    Starting,
    Online,
    Failed,
    Cancelled,
}

impl Phase {
    fn as_str(self) -> &'static str {
        match self {
            Phase::Stopped => "stopped",
            Phase::Probing => "probing",
            Phase::Deploying => "deploying",
            Phase::Starting => "starting",
            Phase::Online => "online",
            Phase::Failed => "failed",
            Phase::Cancelled => "cancelled",
        }
    }
}

struct Slot {
    phase: Phase,
    url: Option<String>,
    remote_url: Option<String>,
    error: Option<String>,
    tunnel: Option<Child>,
    quality: Option<ModelQuality>,
    deployed: bool,
    progress: Option<Progress>,
}

impl Default for Slot {
    fn default() -> Self {
        Self {
            phase: Phase::Stopped,
            url: None,
            remote_url: None,
            error: None,
            tunnel: None,
            quality: None,
            deployed: false,
            progress: None,
        }
    }
}

pub struct Manager {
    sink: EventSink,
    /// Where the desktop app keeps its verified local copy (for uploads).
    local_models: PathBuf,
    slot: Mutex<Slot>,
    /// One deploy/start/stop at a time.
    lifecycle: Mutex<()>,
    enabled: AtomicBool,
    /// Last config used, so shutdown can release the remote process.
    last_config: Mutex<Option<RemoteConfig>>,
    pub cancel: AtomicBool,
    pub deploy_running: AtomicBool,
}

impl Manager {
    pub fn new(local_models: PathBuf, sink: EventSink) -> Self {
        Self {
            sink,
            local_models,
            slot: Mutex::new(Slot::default()),
            lifecycle: Mutex::new(()),
            enabled: AtomicBool::new(false),
            last_config: Mutex::new(None),
            cancel: AtomicBool::new(false),
            deploy_running: AtomicBool::new(false),
        }
    }

    fn remember(&self, cfg: &RemoteConfig) {
        *self.last_config.lock().unwrap() = Some(cfg.clone());
    }

    pub fn set_enabled(&self, enabled: bool) {
        self.enabled.store(enabled, Ordering::SeqCst);
    }

    pub fn is_enabled(&self) -> bool {
        self.enabled.load(Ordering::SeqCst)
    }

    pub fn state(&self) -> State {
        let mut slot = self.slot.lock().unwrap();
        let tunnel = match slot.tunnel.as_mut() {
            Some(child) => matches!(child.try_wait(), Ok(None)),
            None => false,
        };
        State {
            enabled: self.enabled.load(Ordering::SeqCst),
            phase: slot.phase.as_str().to_string(),
            url: slot.url.clone(),
            remote_url: slot.remote_url.clone(),
            tunnel,
            error: slot.error.clone(),
            quality: slot.quality,
            deployed: slot.deployed,
            progress: slot.progress.clone(),
        }
    }

    fn emit(&self, event: Event) {
        (self.sink)(event);
    }

    fn emit_state(&self) {
        self.emit(Event::State(self.state()));
    }

    fn emit_progress(&self, progress: Progress) {
        {
            let mut slot = self.slot.lock().unwrap();
            slot.progress = Some(progress.clone());
        }
        self.emit(Event::Progress(progress));
        self.emit_state();
    }

    fn set_phase(&self, phase: Phase, error: Option<String>) {
        {
            let mut slot = self.slot.lock().unwrap();
            slot.phase = phase;
            slot.error = error;
            if matches!(phase, Phase::Stopped | Phase::Cancelled | Phase::Failed) {
                slot.progress = None;
            }
        }
        self.emit_state();
    }

    /// Inspect the target without changing anything. Safe to call repeatedly.
    /// Serialized with deploy/start so a background probe cannot overwrite the
    /// phase of an in-flight deployment.
    pub fn probe(&self, cfg: &RemoteConfig) -> Result<Probe, Error> {
        validate(cfg)?;
        let _lifecycle = self.lifecycle.lock().unwrap();
        self.cancel.store(false, Ordering::SeqCst);
        self.remember(cfg);
        self.set_phase(Phase::Probing, None);
        let result = self.probe_inner(cfg);
        match &result {
            Ok(probe) => {
                {
                    let mut slot = self.slot.lock().unwrap();
                    slot.quality = Some(cfg.quality);
                    slot.deployed = probe.model_installed;
                }
                self.set_phase(Phase::Stopped, None);
            }
            Err(err) => self.set_phase(Phase::Failed, Some(err.to_string())),
        }
        result
    }

    fn probe_inner(&self, cfg: &RemoteConfig) -> Result<Probe, Error> {
        let script = probe_script(cfg);
        let output = self.run_script(cfg, &script, SSH_TIMEOUT, None)?;
        let probe = parse_probe(&output, &pack_manifest(cfg.quality));
        if !probe.reachable {
            return Err(Error::Ssh(format!(
                "连不上服务器 {}。确认地址、私钥和 ssh-agent，然后重试。",
                destination(cfg)
            )));
        }
        if probe.os != "Linux" {
            return Err(Error::Config(format!(
                "远程部署目前只支持 Linux 服务器，检测到 {}。",
                if probe.os.is_empty() { "未知系统" } else { &probe.os }
            )));
        }
        Ok(probe)
    }

    /// One-click deploy. Runs on a worker thread; progress goes to the sink.
    pub fn deploy(&self, cfg: &RemoteConfig, source: Source) -> Result<(), Error> {
        let _lifecycle = self.lifecycle.lock().unwrap();
        validate(cfg)?;
        self.cancel.store(false, Ordering::SeqCst);
        {
            let mut slot = self.slot.lock().unwrap();
            slot.quality = Some(cfg.quality);
            slot.error = None;
            slot.progress = None;
        }
        self.set_phase(Phase::Deploying, None);
        let outcome = self.deploy_inner(cfg, source);
        match &outcome {
            Ok(()) => {
                self.set_phase(Phase::Online, None);
                self.emit_state();
            }
            Err(Error::Cancelled) => self.set_phase(Phase::Cancelled, None),
            Err(err) => self.set_phase(Phase::Failed, Some(err.to_string())),
        }
        outcome
    }

    fn deploy_inner(&self, cfg: &RemoteConfig, source: Source) -> Result<(), Error> {
        self.progress("probing", Some("正在检查服务器".into()), None, 0, 0);
        let probe = self.probe_inner(cfg)?;
        {
            let mut slot = self.slot.lock().unwrap();
            slot.deployed = probe.model_installed;
        }

        if probe.runtime.is_none() {
            self.ensure_runtime(cfg, &probe)?;
        }
        if !probe.model_installed {
            match source {
                Source::Upload => self.upload_model(cfg, &probe)?,
                Source::Auto | Source::Download => {
                    // Auto picks the server-side download: it avoids pushing
                    // 1–1.4 GB through the SSH link unless the user chooses it.
                    self.download_model(cfg, &probe)?;
                }
            }
        } else if !probe.model_verified {
            match self.verify_model(cfg, &probe) {
                Ok(()) => {}
                // A file with the expected size can still be corrupt. Remove
                // the pack and fetch it again so retrying "部署" is enough.
                Err(Error::Corrupt(_)) => {
                    self.remove_models(cfg, &probe)?;
                    let fresh = self.probe_inner(cfg)?;
                    match source {
                        Source::Upload => self.upload_model(cfg, &fresh)?,
                        Source::Auto | Source::Download => {
                            self.download_model(cfg, &fresh)?;
                        }
                    }
                }
                Err(err) => return Err(err),
            }
        }

        {
            let mut slot = self.slot.lock().unwrap();
            slot.deployed = true;
        }
        self.start_inner(cfg, &probe).map(|_| ())
    }

    /// Reuse a healthy tunnel, otherwise start the remote server and the tunnel.
    pub fn ensure_ready(&self, cfg: &RemoteConfig) -> Result<String, Error> {
        validate(cfg)?;
        self.remember(cfg);
        if let Some(url) = self.online_url() {
            return Ok(url);
        }
        let _lifecycle = self.lifecycle.lock().unwrap();
        // A previous stop/cancel leaves the flag set; holding the lifecycle
        // lock means no deploy is using it now, so this is a fresh start.
        self.cancel.store(false, Ordering::SeqCst);
        if let Some(url) = self.online_url() {
            return Ok(url);
        }
        {
            let mut slot = self.slot.lock().unwrap();
            slot.quality = Some(cfg.quality);
        }
        let probe = self.probe_inner(cfg)?;
        if !probe.model_installed {
            {
                let mut slot = self.slot.lock().unwrap();
                slot.deployed = false;
            }
            return Err(Error::NotDeployed(
                "服务器上还没有识别模型。到设置 → 识别中完成部署。".into(),
            ));
        }
        if probe.runtime.is_none() {
            return Err(Error::NotDeployed(
                "服务器上还没有识别运行时。到设置 → 识别中重新部署。".into(),
            ));
        }
        {
            let mut slot = self.slot.lock().unwrap();
            slot.deployed = true;
        }
        self.set_phase(Phase::Starting, None);
        match self.start_inner(cfg, &probe) {
            Ok(url) => {
                self.set_phase(Phase::Online, None);
                Ok(url)
            }
            Err(err) => {
                self.set_phase(Phase::Failed, Some(err.to_string()));
                Err(err)
            }
        }
    }

    fn online_url(&self) -> Option<String> {
        let (url, online) = {
            let slot = self.slot.lock().unwrap();
            (slot.url.clone(), slot.phase == Phase::Online)
        };
        match url {
            Some(url) if online && health_ok(&url, Duration::from_millis(600)) => Some(url),
            _ => None,
        }
    }

    fn start_inner(&self, cfg: &RemoteConfig, probe: &Probe) -> Result<String, Error> {
        let url = self.start_tunnel(cfg)?;
        let _ = probe;
        // A server started by an earlier app session can still be healthy; in
        // that case skip the process restart and reuse it.
        let healthy = self.remote_healthy(cfg).unwrap_or(false);
        if !healthy {
            self.progress("starting", Some("正在启动识别服务".into()), None, 0, 0);
            let script = start_script(cfg, probe);
            let output = self.run_script(cfg, &script, SSH_TIMEOUT, None)?;
            if output.contains("START_ERROR") {
                self.stop_tunnel();
                return Err(Error::Remote(format!(
                    "识别服务没有启动起来。\n{}",
                    tail_text(&output)
                )));
            }
        }

        let started = Instant::now();
        while started.elapsed() < STARTUP_TIMEOUT {
            if self.cancel.load(Ordering::SeqCst) {
                self.stop_tunnel();
                return Err(Error::Cancelled);
            }
            if let Some(mut tunnel) = self.tunnel_exited() {
                let _ = tunnel.wait();
                return Err(Error::Ssh(
                    "SSH 隧道断开了。检查服务器连接，或换一个本地端口。".into(),
                ));
            }
            if health_ok(&url, Duration::from_millis(700)) {
                {
                    let mut slot = self.slot.lock().unwrap();
                    slot.url = Some(url.clone());
                    slot.remote_url = Some(format!("http://127.0.0.1:{}", cfg.server_port));
                }
                self.progress("ready", Some("识别服务已就绪".into()), None, 0, 0);
                return Ok(url);
            }
            thread::sleep(Duration::from_millis(400));
        }
        self.stop_tunnel();
        Err(Error::Timeout(
            "识别服务在 240 秒内没有就绪。查看服务器日志后重试。".into(),
        ))
    }

    fn tunnel_exited(&self) -> Option<Child> {
        let mut slot = self.slot.lock().unwrap();
        let exited = match slot.tunnel.as_mut() {
            Some(child) => matches!(child.try_wait(), Ok(Some(_))),
            None => true,
        };
        if exited {
            slot.tunnel.take()
        } else {
            None
        }
    }

    fn start_tunnel(&self, cfg: &RemoteConfig) -> Result<String, Error> {
        self.stop_tunnel();
        let local_port = if cfg.local_port == 0 {
            pick_local_port()
        } else {
            cfg.local_port
        };
        let url = format!("http://127.0.0.1:{local_port}");
        let mut cmd = ssh_base(cfg)?;
        cmd.arg("-o")
            .arg("ExitOnForwardFailure=yes")
            .arg("-N")
            .arg("-L")
            .arg(format!(
                "127.0.0.1:{local_port}:127.0.0.1:{}",
                cfg.server_port
            ))
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        let child = cmd
            .spawn()
            .map_err(|e| Error::Ssh(format!("无法启动 SSH 隧道：{e}")))?;
        {
            let mut slot = self.slot.lock().unwrap();
            slot.tunnel = Some(child);
            slot.url = Some(url.clone());
        }
        self.emit_state();
        Ok(url)
    }

    fn stop_tunnel(&self) {
        let child = {
            let mut slot = self.slot.lock().unwrap();
            slot.tunnel.take()
        };
        if let Some(mut child) = child {
            let _ = child.kill();
            let _ = child.wait();
        }
        let mut slot = self.slot.lock().unwrap();
        slot.url = None;
        slot.remote_url = None;
    }

    /// Stop the tunnel and the remote process. Best effort: a stop must not
    /// block quitting the app for long.
    pub fn stop(&self) {
        // Signal an in-flight download/start before waiting for the lifecycle
        // lock, otherwise a long ensure_ready can block Settings for minutes.
        self.cancel.store(true, Ordering::SeqCst);
        let _lifecycle = self.lifecycle.lock().unwrap();
        self.stop_tunnel();
        self.set_phase(Phase::Stopped, None);
    }

    /// Stop with an explicit config so the remote process is released too.
    pub fn stop_with(&self, cfg: &RemoteConfig) {
        self.cancel.store(true, Ordering::SeqCst);
        let _lifecycle = self.lifecycle.lock().unwrap();
        self.remember(cfg);
        self.stop_tunnel();
        if validate(cfg).is_ok() {
            let script = stop_script(cfg);
            let _ = self.run_script(cfg, &script, Duration::from_secs(12), None);
        }
        self.set_phase(Phase::Stopped, None);
    }

    pub fn shutdown(&self) {
        self.cancel.store(true, Ordering::SeqCst);
        self.stop_tunnel();
        let cfg = self.last_config.lock().unwrap().clone();
        if let Some(cfg) = cfg {
            if validate(&cfg).is_ok() {
                let script = stop_script(&cfg);
                let _ = self.run_script(&cfg, &script, Duration::from_secs(5), None);
            }
        }
    }

    // -- helpers -----------------------------------------------------------

    fn progress(
        &self,
        phase: &str,
        message: Option<String>,
        file: Option<String>,
        downloaded: u64,
        total: u64,
    ) {
        self.emit_progress(Progress {
            phase: phase.to_string(),
            message,
            file,
            downloaded,
            total,
            bytes_per_sec: 0,
            eta_secs: None,
        });
    }

    fn progress_bytes(&self, phase: &str, file: &str, downloaded: u64, total: u64, started: Instant) {
        let elapsed = started.elapsed().as_secs_f64().max(0.001);
        let speed = (downloaded as f64 / elapsed) as u64;
        let eta = if speed > 0 && total > downloaded {
            Some((total - downloaded) / speed)
        } else {
            None
        };
        self.emit_progress(Progress {
            phase: phase.to_string(),
            message: Some(if phase == "uploading" {
                format!("正在上传 {file}")
            } else {
                format!("正在服务器下载 {file}")
            }),
            file: Some(file.to_string()),
            downloaded,
            total,
            bytes_per_sec: speed,
            eta_secs: eta,
        });
    }

    fn ensure_runtime(&self, cfg: &RemoteConfig, probe: &Probe) -> Result<(), Error> {
        if !probe.tar {
            return Err(Error::Remote("服务器缺少 tar，无法解压运行时。".into()));
        }
        if !probe.curl && !probe.wget {
            return Err(Error::Remote(
                "服务器缺少 curl 或 wget，无法下载运行时。".into(),
            ));
        }
        let assets = runtime_assets(&probe.arch, cfg.accel)?;
        self.progress(
            "runtime",
            Some("正在安装识别运行时".into()),
            None,
            0,
            0,
        );
        let mut script = String::from("set -u\n");
        script.push_str(&format!("DIR={}\n", shell_quote(&probe.dir)));
        script.push_str("mkdir -p \"$DIR/runtime\" \"$DIR/runtime.tmp\"\n");
        script.push_str("rm -rf \"$DIR/runtime.tmp\"/*\n");
        for (index, url) in assets.iter().enumerate() {
            let archive = format!("$DIR/runtime-{index}.tar.gz");
            script.push_str(&download_function());
            script.push_str(&format!(
                "fetch {} {} || {{ echo OCR4CODE FETCH_FAILED=1; exit 3; }}\n",
                shell_quote(url),
                archive
            ));
            script.push_str(&format!(
                "tar -xzf {} -C \"$DIR/runtime.tmp\" || {{ echo OCR4CODE UNTAR_FAILED=1; exit 3; }}\n",
                archive
            ));
        }
        script.push_str(
            "find \"$DIR/runtime.tmp\" \\( -type f -o -type l \\) -exec cp -a {} \"$DIR/runtime/\" \\; 2>/dev/null\n\
             chmod +x \"$DIR/runtime/llama-server\" 2>/dev/null\n\
             rm -rf \"$DIR/runtime.tmp\" \"$DIR/runtime-\"*.tar.gz\n\
             if [ ! -x \"$DIR/runtime/llama-server\" ]; then echo OCR4CODE RUNTIME_MISSING=1; exit 4; fi\n\
             echo OCR4CODE RUNTIME_OK=1\n",
        );
        let output = self.run_script(cfg, &script, DOWNLOAD_TIMEOUT, None)?;
        if output.contains("FETCH_FAILED") || output.contains("UNTAR_FAILED") {
            return Err(Error::Remote(
                "运行时下载失败。检查服务器的网络，或换用其他加速方式。".into(),
            ));
        }
        if output.contains("RUNTIME_MISSING") {
            return Err(Error::Remote(
                "运行时压缩包里没有 llama-server，请重试或手动安装。".into(),
            ));
        }
        Ok(())
    }

    fn download_model(&self, cfg: &RemoteConfig, probe: &Probe) -> Result<(), Error> {
        if !probe.curl && !probe.wget {
            return Err(Error::Remote(
                "服务器缺少 curl 或 wget，无法下载模型。".into(),
            ));
        }
        let files = model_files(cfg.quality);
        for file in files {
            let expected = probe
                .files
                .iter()
                .find(|item| item.name == file.name)
                .map(|item| item.bytes)
                .unwrap_or(0);
            if expected == file.bytes {
                continue;
            }
            let url = model_url(file, cfg.use_mirror);
            let target = format!("{}/models/{}", probe.dir, file.name);
            let part = format!("{target}.part");
            self.progress_bytes("downloading", file.name, 0, file.bytes, Instant::now());
            let script = format!(
                "set -u\nDIR={dir}\nmkdir -p \"$DIR/models\"\n{download}\n\
                 fetch {url} {part} || {{ echo OCR4CODE FETCH_FAILED=1; exit 3; }}\n\
                 mv -f {part} {target}\n\
                 echo OCR4CODE FILE_OK=1\n",
                dir = shell_quote(&probe.dir),
                download = download_function(),
                url = shell_quote(&url),
                part = shell_quote(&part),
                target = shell_quote(&target),
            );
            let started = Instant::now();
            let output = self.run_script(cfg, &script, DOWNLOAD_TIMEOUT, Some((part.as_str(), file.name, file.bytes)))?;
            if output.contains("FETCH_FAILED") {
                return Err(Error::Remote(format!(
                    "下载 {} 失败。检查服务器网络，或开启下载镜像。",
                    file.name
                )));
            }
            self.progress_bytes("downloading", file.name, file.bytes, file.bytes, started);
        }
        self.verify_model(cfg, probe)
    }

    fn upload_model(&self, cfg: &RemoteConfig, probe: &Probe) -> Result<(), Error> {
        let mut verified = false;
        for file in model_files(cfg.quality) {
            let local = self.local_models.join(file.name);
            let meta = fs::metadata(&local).map_err(|_| {
                Error::Config(format!(
                    "本机没有 {}。先在服务器下载，或在本机下载后再上传。",
                    file.name
                ))
            })?;
            if meta.len() != file.bytes {
                return Err(Error::Config(format!(
                    "本机的 {} 大小不一致，重新下载后再上传。",
                    file.name
                )));
            }
            let remote_present = probe
                .files
                .iter()
                .find(|item| item.name == file.name)
                .map(|item| item.bytes == file.bytes)
                .unwrap_or(false);
            if remote_present {
                continue;
            }
            let target = format!("{}/models/{}", probe.dir, file.name);
            let part = format!("{target}.part");
            self.progress_bytes("uploading", file.name, 0, file.bytes, Instant::now());
            self.stream_file(cfg, &local, &part, &target, file.name, file.bytes)?;
            verified = true;
        }
        if verified {
            self.verify_model(cfg, probe)?;
        }
        Ok(())
    }

    /// Stream a local file into `cat > part` on the server, polling the remote
    /// size so the UI gets the same progress model as a server-side download.
    fn stream_file(
        &self,
        cfg: &RemoteConfig,
        local: &Path,
        part: &str,
        target: &str,
        name: &str,
        total: u64,
    ) -> Result<(), Error> {
        let mut cmd = ssh_base(cfg)?;
        // ssh joins its arguments with spaces and lets the remote shell parse
        // the result. Quote the whole `sh -c` payload so `&&` and the path
        // quotes survive that second round of parsing.
        cmd.arg("sh")
            .arg("-c")
            .arg(shell_quote(&upload_command(part)))
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        let mut child = cmd
            .spawn()
            .map_err(|e| Error::Ssh(format!("无法启动上传通道：{e}")))?;
        let mut stdin = child
            .stdin
            .take()
            .ok_or_else(|| Error::Ssh("上传通道没有 stdin".into()))?;
        let mut file = File::open(local)
            .map_err(|e| Error::Config(format!("读取 {} 失败：{e}", local.display())))?;
        let started = Instant::now();
        let mut sent = 0u64;
        let mut buffer = vec![0u8; 512 * 1024];
        let mut last_emit = Instant::now();
        loop {
            if self.cancel.load(Ordering::SeqCst) {
                drop(stdin);
                let _ = child.kill();
                let _ = child.wait();
                return Err(Error::Cancelled);
            }
            let read = file
                .read(&mut buffer)
                .map_err(|e| Error::Config(format!("读取 {} 失败：{e}", local.display())))?;
            if read == 0 {
                break;
            }
            if stdin.write_all(&buffer[..read]).is_err() {
                let _ = child.wait();
                return Err(Error::Ssh("上传中断，检查服务器连接。".into()));
            }
            sent += read as u64;
            if last_emit.elapsed() >= Duration::from_secs(2) {
                self.progress_bytes("uploading", name, sent, total, started);
                last_emit = Instant::now();
            }
        }
        drop(stdin);
        let status = child
            .wait()
            .map_err(|e| Error::Ssh(format!("上传没有完成：{e}")))?;
        if !status.success() {
            return Err(Error::Ssh("上传中断，检查服务器连接。".into()));
        }
        self.progress_bytes("uploading", name, total, total, started);
        let script = format!(
            "set -e\nmv -f {} {}\necho OCR4CODE FILE_OK=1\n",
            shell_quote(part),
            shell_quote(target)
        );
        self.run_script(cfg, &script, SSH_TIMEOUT, None)?;
        Ok(())
    }

    fn verify_model(&self, cfg: &RemoteConfig, probe: &Probe) -> Result<(), Error> {
        if probe.sha256.is_empty() {
            return Err(Error::Remote(
                "服务器缺少 sha256sum/shasum，无法校验模型。安装 coreutils 后重试。".into(),
            ));
        }
        self.progress("verifying", Some("正在校验模型".into()), None, 0, 0);
        let tool = if probe.sha256 == "shasum" {
            "shasum -a 256"
        } else {
            "sha256sum"
        };
        let mut script = format!(
            "cd {}\n",
            shell_quote(&format!("{}/models", probe.dir))
        );
        for file in model_files(cfg.quality) {
            let name = file.name;
            let quoted = shell_quote(name);
            script.push_str(&format!(
                "if [ -f {quoted} ]; then echo \"OCR4CODE HASH={name}:$({tool} {quoted} | awk '{{print $1}}')\"; else echo \"OCR4CODE MISSING={name}\"; fi\n",
            ));
        }
        let output = self.run_script(cfg, &script, DOWNLOAD_TIMEOUT, None)?;
        let hashes = parse_hashes(&output);
        for file in model_files(cfg.quality) {
            match hashes.get(file.name) {
                Some(hash) if hash.eq_ignore_ascii_case(file.sha256) => {}
                Some(_) => {
                    return Err(Error::Corrupt(format!(
                        "{} 校验失败，文件可能不完整。重新下载后重试。",
                        file.name
                    )))
                }
                None => {
                    return Err(Error::Remote(format!(
                        "服务器上缺少 {}，重新部署后重试。",
                        file.name
                    )))
                }
            }
        }
        let marker = format!(
            "{}/models/verified-{}.json",
            probe.dir,
            cfg.quality.as_str()
        );
        let script = format!(
            "printf '%s' {} > {}\necho OCR4CODE VERIFIED=1\n",
            shell_quote(&pack_manifest(cfg.quality)),
            shell_quote(&marker)
        );
        self.run_script(cfg, &script, SSH_TIMEOUT, None)?;
        Ok(())
    }

    fn remove_models(&self, cfg: &RemoteConfig, probe: &Probe) -> Result<(), Error> {
        let models = format!("{}/models", probe.dir);
        let mut paths: Vec<String> = model_files(cfg.quality)
            .iter()
            .map(|file| shell_quote(&format!("{models}/{}", file.name)))
            .collect();
        paths.push(shell_quote(&format!(
            "{models}/verified-{}.json",
            cfg.quality.as_str()
        )));
        let script = format!("set -e\nrm -f {}\necho OCR4CODE REMOVED=1\n", paths.join(" "));
        self.run_script(cfg, &script, SSH_TIMEOUT, None)?;
        Ok(())
    }

    fn remote_healthy(&self, cfg: &RemoteConfig) -> Result<bool, Error> {
        let script = format!(
            "if command -v curl >/dev/null 2>&1; then curl -fsS --max-time 2 http://127.0.0.1:{}/health >/dev/null 2>&1 && echo OCR4CODE HEALTHY=1; else exit 7; fi\n",
            cfg.server_port
        );
        let output = self.run_script(cfg, &script, Duration::from_secs(12), None)?;
        Ok(output.contains("HEALTHY=1"))
    }

    fn run_script(
        &self,
        cfg: &RemoteConfig,
        script: &str,
        timeout: Duration,
        poll: Option<(&str, &str, u64)>,
    ) -> Result<String, Error> {
        let mut cmd = ssh_base(cfg)?;
        cmd.arg("sh")
            .arg("-s")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = cmd
            .spawn()
            .map_err(|e| Error::Ssh(format!("无法启动 ssh：{e}")))?;
        if let Some(mut stdin) = child.stdin.take() {
            stdin
                .write_all(script.as_bytes())
                .map_err(|e| Error::Ssh(format!("发送部署脚本失败：{e}")))?;
        }
        let stdout = child.stdout.take().map(|mut pipe| {
            thread::spawn(move || {
                let mut buffer = Vec::new();
                let _ = pipe.read_to_end(&mut buffer);
                buffer
            })
        });
        let stderr = child.stderr.take().map(|mut pipe| {
            thread::spawn(move || {
                let mut buffer = Vec::new();
                let _ = pipe.read_to_end(&mut buffer);
                buffer
            })
        });
        let started = Instant::now();
        let mut last_poll = Instant::now();
        let mut last_bytes = 0u64;
        let mut last_bytes_at = Instant::now();
        let status = loop {
            if self.cancel.load(Ordering::SeqCst) {
                let _ = child.kill();
                let _ = child.wait();
                return Err(Error::Cancelled);
            }
            if started.elapsed() > timeout {
                let _ = child.kill();
                let _ = child.wait();
                return Err(Error::Timeout(
                    "远程操作超时，检查服务器和网络后重试。".into(),
                ));
            }
            match child.try_wait() {
                Ok(Some(status)) => break status,
                Ok(None) => {}
                Err(e) => {
                    let _ = child.kill();
                    return Err(Error::Ssh(format!("等待 ssh 失败：{e}")));
                }
            }
            if let Some((path, file, total)) = poll {
                if last_poll.elapsed() >= Duration::from_secs(2) {
                    let bytes = self.remote_file_size(cfg, path).unwrap_or(0);
                    last_poll = Instant::now();
                    if bytes != last_bytes {
                        let elapsed = last_bytes_at.elapsed().as_secs_f64().max(0.001);
                        let speed = ((bytes.saturating_sub(last_bytes)) as f64 / elapsed) as u64;
                        let eta = if speed > 0 && total > bytes {
                            Some((total - bytes) / speed)
                        } else {
                            None
                        };
                        self.emit_progress(Progress {
                            phase: "downloading".to_string(),
                            message: Some(format!("正在服务器下载 {file}")),
                            file: Some(file.to_string()),
                            downloaded: bytes,
                            total,
                            bytes_per_sec: speed,
                            eta_secs: eta,
                        });
                        last_bytes = bytes;
                        last_bytes_at = Instant::now();
                    }
                }
            }
            thread::sleep(Duration::from_millis(250));
        };
        let out = stdout
            .map(|handle| handle.join().unwrap_or_default())
            .unwrap_or_default();
        let err = stderr
            .map(|handle| handle.join().unwrap_or_default())
            .unwrap_or_default();
        if !status.success() {
            let detail = String::from_utf8_lossy(&err).trim().to_string();
            return Err(Error::Remote(if detail.is_empty() {
                "服务器命令执行失败。检查服务器环境后重试。".into()
            } else {
                format!("服务器命令执行失败：{detail}")
            }));
        }
        Ok(String::from_utf8_lossy(&out).to_string())
    }

    fn remote_file_size(&self, cfg: &RemoteConfig, path: &str) -> Result<u64, Error> {
        let mut cmd = ssh_base(cfg)?;
        cmd.arg(format!(
            "stat -c %s {} 2>/dev/null || echo 0",
            shell_quote(path)
        ))
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
        let output = cmd
            .output()
            .map_err(|e| Error::Ssh(format!("查询文件大小失败：{e}")))?;
        let text = String::from_utf8_lossy(&output.stdout);
        Ok(text.trim().parse::<u64>().unwrap_or(0))
    }
}

impl Drop for Manager {
    fn drop(&mut self) {
        if let Ok(mut slot) = self.slot.lock() {
            if let Some(mut child) = slot.tunnel.take() {
                let _ = child.kill();
                let _ = child.wait();
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Scripts and parsing
// ---------------------------------------------------------------------------

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn parent_dir(path: &str) -> &str {
    match path.rfind('/') {
        Some(index) if index > 0 => &path[..index],
        _ => "/",
    }
}

/// The remote payload for `ssh host sh -c <quoted payload>`. Kept separate so
/// the quoting can be exercised without a server.
fn upload_command(part: &str) -> String {
    format!(
        "mkdir -p {} && cat > {}",
        shell_quote(parent_dir(part)),
        shell_quote(part)
    )
}

fn expand_local_path(value: &str) -> String {
    if let Some(rest) = value.strip_prefix("~/") {
        if let Ok(home) = std::env::var("HOME") {
            return format!("{home}/{rest}");
        }
    }
    value.to_string()
}

fn destination(cfg: &RemoteConfig) -> String {
    format!("{}@{}", cfg.user, cfg.host)
}

fn ssh_base(cfg: &RemoteConfig) -> Result<Command, Error> {
    validate(cfg)?;
    let mut cmd = Command::new("ssh");
    cmd.arg("-o")
        .arg("BatchMode=yes")
        .arg("-o")
        .arg("ConnectTimeout=10")
        .arg("-o")
        .arg("StrictHostKeyChecking=accept-new")
        .arg("-o")
        .arg("ServerAliveInterval=15")
        .arg("-o")
        .arg("ServerAliveCountMax=3")
        .arg("-p")
        .arg(cfg.ssh_port.to_string());
    let key = cfg.key_path.trim();
    if !key.is_empty() {
        cmd.arg("-i").arg(expand_local_path(key));
    }
    cmd.arg(destination(cfg));
    Ok(cmd)
}

pub fn validate(cfg: &RemoteConfig) -> Result<(), Error> {
    let host = cfg.host.trim();
    if host.is_empty() {
        return Err(Error::Config("服务器地址不能为空".into()));
    }
    if host.starts_with('-')
        || !host
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_' | ':' | '[' | ']'))
    {
        return Err(Error::Config("服务器地址只能用主机名或 IP".into()));
    }
    let user = cfg.user.trim();
    if user.is_empty()
        || user.starts_with('-')
        || !user
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))
    {
        return Err(Error::Config("用户名无效".into()));
    }
    if cfg.ssh_port == 0 || cfg.server_port == 0 {
        return Err(Error::Config("端口必须在 1–65535 之间".into()));
    }
    let dir = cfg.dir.trim();
    if dir.is_empty() || dir.contains('\n') || dir.contains('\0') {
        return Err(Error::Config("远程目录不能为空".into()));
    }
    if cfg.gpu_layers > 999 {
        return Err(Error::Config("GPU 层数应在 0–999 之间".into()));
    }
    Ok(())
}

fn probe_script(cfg: &RemoteConfig) -> String {
    let mut script = String::from(
        "set -u\n\
         DIR_RAW=",
    );
    script.push_str(&shell_quote(cfg.dir.trim()));
    script.push_str(
        "\n\
         case \"$DIR_RAW\" in\n\
           \"~\") DIR=\"$HOME\" ;;\n\
           \"~/\"*) DIR=\"$HOME/${DIR_RAW#\\~/}\" ;;\n\
           *) DIR=\"$DIR_RAW\" ;;\n\
         esac\n\
         mkdir -p \"$DIR/models\" \"$DIR/logs\" 2>/dev/null || true\n\
         echo \"OCR4CODE OS=$(uname -s 2>/dev/null)\"\n\
         echo \"OCR4CODE ARCH=$(uname -m 2>/dev/null)\"\n\
         echo \"OCR4CODE HOME=$HOME\"\n\
         echo \"OCR4CODE DIR=$DIR\"\n\
         FREE=$(df -Pk \"$DIR\" 2>/dev/null | awk 'NR==2 {print $4}')\n\
         [ -n \"${FREE:-}\" ] && echo \"OCR4CODE DISK=$((FREE * 1024))\"\n\
         command -v curl >/dev/null 2>&1 && echo \"OCR4CODE CURL=1\"\n\
         command -v wget >/dev/null 2>&1 && echo \"OCR4CODE WGET=1\"\n\
         command -v tar >/dev/null 2>&1 && echo \"OCR4CODE TAR=1\"\n\
         if command -v sha256sum >/dev/null 2>&1; then echo \"OCR4CODE SHA256=sha256sum\";\n\
         elif command -v shasum >/dev/null 2>&1; then echo \"OCR4CODE SHA256=shasum\"; fi\n\
         if [ -x \"$DIR/runtime/llama-server\" ]; then echo \"OCR4CODE RUNTIME=$DIR/runtime/llama-server\";\n\
         elif command -v llama-server >/dev/null 2>&1; then echo \"OCR4CODE RUNTIME=$(command -v llama-server)\"; fi\n",
    );
    for file in model_files(cfg.quality) {
        script.push_str(&format!(
            "if [ -f \"$DIR/models/\"{name} ]; then GOT=$(stat -c %s \"$DIR/models/\"{name} 2>/dev/null || echo 0); else GOT=0; fi\n\
             echo \"OCR4CODE FILE={name}:$GOT:{bytes}\"\n",
            name = shell_quote(file.name),
            bytes = file.bytes
        ));
    }
    script.push_str(&format!(
        "MARKER=\"$DIR/models/verified-{}.json\"\n\
         if [ -f \"$MARKER\" ]; then echo \"OCR4CODE MARKER=$(cat \"$MARKER\" 2>/dev/null | tr -d '\\n')\"; fi\n\
         echo OCR4CODE DONE=1\n",
        cfg.quality.as_str()
    ));
    script
}

fn download_function() -> String {
    "fetch() {\n\
       if command -v curl >/dev/null 2>&1; then\n\
         curl -fL --retry 3 -C - --no-progress-meter -o \"$2\" \"$1\" || \\\n\
           curl -fL --retry 3 --no-progress-meter -o \"$2\" \"$1\";\n\
       elif command -v wget >/dev/null 2>&1; then\n\
         wget -q -c -O \"$2\" \"$1\";\n\
       else\n\
         return 1;\n\
       fi\n\
     }\n"
        .to_string()
}

fn start_script(cfg: &RemoteConfig, probe: &Probe) -> String {
    let files = model_files(cfg.quality);
    let model = format!("{}/models/{}", probe.dir, files[0].name);
    let mmproj = format!("{}/models/{}", probe.dir, files[1].name);
    let runtime = probe
        .runtime
        .clone()
        .unwrap_or_else(|| format!("{}/runtime/llama-server", probe.dir));
    // A system llama-server may predate the pinned build; only the runtime we
    // installed is guaranteed to understand the newer memory flags.
    let bundled = runtime.starts_with(&format!("{}/runtime/", probe.dir));
    let args = server_args(cfg, &model, &mmproj, cfg.server_port, bundled);
    let quoted_args = args
        .iter()
        .map(|arg| shell_quote(arg))
        .collect::<Vec<_>>()
        .join(" ");
    let mut script = String::from("set -u\n");
    script.push_str(&format!("DIR={}\n", shell_quote(&probe.dir)));
    script.push_str(&format!("RUNTIME={}\n", shell_quote(&runtime)));
    script.push_str(&format!(
        "PIDFILE=\"$DIR/llama-server.pid\"\nLOG=\"$DIR/logs/llama-server.log\"\nmkdir -p \"$DIR/logs\" \"$DIR/models\"\n\
         if [ -f \"$PIDFILE\" ]; then\n\
           OLD=$(cat \"$PIDFILE\" 2>/dev/null || true)\n\
           if [ -n \"${{OLD:-}}\" ] && kill -0 \"$OLD\" 2>/dev/null; then\n\
             kill \"$OLD\" 2>/dev/null || true\n\
             I=0; while [ $I -lt 40 ] && kill -0 \"$OLD\" 2>/dev/null; do sleep 0.1; I=$((I+1)); done\n\
             kill -9 \"$OLD\" 2>/dev/null || true\n\
           fi\n\
           rm -f \"$PIDFILE\"\n\
         fi\n\
         if [ ! -x \"$RUNTIME\" ] && ! command -v \"$RUNTIME\" >/dev/null 2>&1; then echo OCR4CODE START_ERROR=1; exit 4; fi\n\
         nohup \"$RUNTIME\" {args} </dev/null >>\"$LOG\" 2>&1 &\n\
         NEW=$!\n\
         echo \"$NEW\" > \"$PIDFILE\"\n\
         sleep 0.6\n\
         if ! kill -0 \"$NEW\" 2>/dev/null; then\n\
           echo OCR4CODE START_ERROR=1\n\
           tail -n 20 \"$LOG\" 2>/dev/null\n\
           exit 4\n\
         fi\n\
         echo \"OCR4CODE PID=$NEW\"\n",
        args = quoted_args
    ));
    script
}

fn stop_script(cfg: &RemoteConfig) -> String {
    let mut script = String::from("set -u\n");
    script.push_str(&format!("DIR_RAW={}\n", shell_quote(cfg.dir.trim())));
    script.push_str(
        "case \"$DIR_RAW\" in\n\
           \"~\") DIR=\"$HOME\" ;;\n\
           \"~/\"*) DIR=\"$HOME/${DIR_RAW#\\~/}\" ;;\n\
           *) DIR=\"$DIR_RAW\" ;;\n\
         esac\n\
         PIDFILE=\"$DIR/llama-server.pid\"\n\
         if [ -f \"$PIDFILE\" ]; then\n\
           PID=$(cat \"$PIDFILE\" 2>/dev/null || true)\n\
           if [ -n \"${PID:-}\" ] && kill -0 \"$PID\" 2>/dev/null; then\n\
             kill \"$PID\" 2>/dev/null || true\n\
             I=0; while [ $I -lt 40 ] && kill -0 \"$PID\" 2>/dev/null; do sleep 0.1; I=$((I+1)); done\n\
             kill -9 \"$PID\" 2>/dev/null || true\n\
           fi\n\
           rm -f \"$PIDFILE\"\n\
         fi\n\
         echo OCR4CODE STOPPED=1\n",
    );
    script
}

fn server_args(
    cfg: &RemoteConfig,
    model: &str,
    mmproj: &str,
    port: u16,
    bundled: bool,
) -> Vec<String> {
    let slots = llama_slots(cfg.jobs);
    let ctx = total_ctx(cfg.jobs, cfg.max_tokens);
    let mut args = vec![
        "-m".to_string(),
        model.to_string(),
        "--mmproj".to_string(),
        mmproj.to_string(),
        "--host".to_string(),
        "127.0.0.1".to_string(),
        "--port".to_string(),
        port.to_string(),
        "-c".to_string(),
        ctx.to_string(),
        "-np".to_string(),
        slots.to_string(),
        "-ngl".to_string(),
        if cfg.accel == Accel::Cpu || cfg.accel == Accel::Auto {
            "0".to_string()
        } else {
            cfg.gpu_layers.to_string()
        },
    ];
    if bundled {
        args.push("-cram".to_string());
        args.push("0".to_string());
        if cfg.idle_sleep_secs > 0 {
            args.push("--sleep-idle-seconds".to_string());
            args.push(cfg.idle_sleep_secs.to_string());
        }
    }
    if cfg.kv_cache_quant {
        args.push("-ctk".to_string());
        args.push("q8_0".to_string());
        args.push("-ctv".to_string());
        args.push("q8_0".to_string());
    }
    args
}

fn runtime_assets(arch: &str, accel: Accel) -> Result<Vec<String>, Error> {
    let arch = match arch.trim() {
        "x86_64" | "amd64" => "x64",
        "aarch64" | "arm64" => "arm64",
        other => {
            return Err(Error::Config(format!(
                "服务器架构 {other} 没有可用的 llama.cpp 预编译包。"
            )))
        }
    };
    let names: Vec<String> = match accel {
        Accel::Auto | Accel::Cpu => vec![format!(
            "llama-{RUNTIME_TAG}-bin-ubuntu-{arch}.tar.gz"
        )],
        Accel::Vulkan => vec![format!(
            "llama-{RUNTIME_TAG}-bin-ubuntu-vulkan-{arch}.tar.gz"
        )],
        Accel::Cuda => {
            if arch != "x64" {
                return Err(Error::Config(
                    "CUDA 运行时目前只有 x64 包；arm64 服务器请用 Vulkan 或 CPU。".into(),
                ));
            }
            vec![
                format!("llama-{RUNTIME_TAG}-bin-ubuntu-cuda-13.4-x64.tar.gz"),
                format!("cudart-llama-{RUNTIME_TAG}-bin-ubuntu-cuda-13.4-x64.tar.gz"),
            ]
        }
    };
    Ok(names
        .into_iter()
        .map(|name| format!("{RUNTIME_BASE}/{RUNTIME_TAG}/{name}"))
        .collect())
}

fn parse_probe(output: &str, expected_marker: &str) -> Probe {
    let mut probe = Probe::default();
    for line in output.lines() {
        let Some(rest) = line.strip_prefix("OCR4CODE ") else {
            continue;
        };
        let Some((key, value)) = rest.split_once('=') else {
            continue;
        };
        match key {
            "OS" => {
                probe.os = value.to_string();
                probe.reachable = true;
            }
            "ARCH" => probe.arch = value.to_string(),
            "HOME" => probe.home = value.to_string(),
            "DIR" => probe.dir = value.to_string(),
            "DISK" => probe.disk_free = value.parse().unwrap_or(0),
            "CURL" => probe.curl = value == "1",
            "WGET" => probe.wget = value == "1",
            "TAR" => probe.tar = value == "1",
            "SHA256" => probe.sha256 = value.to_string(),
            "RUNTIME" => probe.runtime = Some(value.to_string()),
            "MARKER" => probe.model_verified = value == expected_marker,
            "FILE" => {
                let mut parts = value.splitn(3, ':');
                let name = parts.next().unwrap_or_default().to_string();
                let bytes = parts.next().and_then(|v| v.parse().ok()).unwrap_or(0);
                let expected = parts.next().and_then(|v| v.parse().ok()).unwrap_or(0);
                probe.files.push(ProbeFile {
                    present: bytes == expected && expected > 0,
                    name,
                    bytes,
                    expected,
                });
            }
            _ => {}
        }
    }
    probe.model_installed = !probe.files.is_empty() && probe.files.iter().all(|file| file.present);
    probe
}

fn parse_hashes(output: &str) -> std::collections::HashMap<String, String> {
    let mut hashes = std::collections::HashMap::new();
    for line in output.lines() {
        let Some(rest) = line.strip_prefix("OCR4CODE HASH=") else {
            continue;
        };
        if let Some((name, hash)) = rest.split_once(':') {
            hashes.insert(name.to_string(), hash.to_string());
        }
    }
    hashes
}

fn tail_text(output: &str) -> String {
    let lines: Vec<&str> = output
        .lines()
        .filter(|line| !line.starts_with("OCR4CODE"))
        .collect();
    let start = lines.len().saturating_sub(12);
    lines[start..].join("\n")
}

fn pick_local_port() -> u16 {
    for port in LOCAL_PORT_START..=LOCAL_PORT_END {
        if TcpListener::bind(("127.0.0.1", port)).is_ok() {
            return port;
        }
    }
    TcpListener::bind(("127.0.0.1", 0))
        .and_then(|listener| listener.local_addr())
        .map(|addr| addr.port())
        .unwrap_or(LOCAL_PORT_START)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config() -> RemoteConfig {
        RemoteConfig {
            host: "server.example".into(),
            user: "dev".into(),
            ssh_port: 22,
            key_path: String::new(),
            dir: "~/.ocr4code".into(),
            server_port: 8099,
            local_port: 0,
            accel: Accel::Auto,
            gpu_layers: 99,
            quality: ModelQuality::Low,
            source: Source::Auto,
            jobs: 3,
            max_tokens: 3072,
            kv_cache_quant: true,
            idle_sleep_secs: 300,
            use_mirror: false,
        }
    }

    #[test]
    fn shell_quotes_single_quotes_safely() {
        assert_eq!(shell_quote("plain"), "'plain'");
        assert_eq!(shell_quote("it's"), "'it'\\''s'");
        assert_eq!(shell_quote("$HOME `x`"), "'$HOME `x`'");
    }

    #[cfg(unix)]
    #[test]
    fn upload_payload_creates_the_part_file_through_a_remote_shell() {
        use std::io::Write as _;

        let root = std::env::temp_dir().join(format!(
            "ocr4code-upload-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        let part = root.join("models").join("model name.gguf.part");
        // ssh joins its argv and lets the remote shell parse the result; this
        // `sh -c` hop reproduces that second round of parsing.
        let remote = format!("sh -c {}", shell_quote(&upload_command(part.to_str().unwrap())));
        let mut child = Command::new("sh")
            .arg("-c")
            .arg(&remote)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(b"weights")
            .unwrap();
        assert!(child.wait().unwrap().success());
        assert_eq!(fs::read(&part).unwrap(), b"weights");
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn destination_is_user_at_host() {
        assert_eq!(destination(&config()), "dev@server.example");
    }

    #[test]
    fn validation_rejects_option_injection() {
        let mut cfg = config();
        assert!(validate(&cfg).is_ok());
        cfg.host = "-oProxyCommand=evil".into();
        assert!(validate(&cfg).is_err());
        cfg.host = "good.example".into();
        cfg.user = "-root".into();
        assert!(validate(&cfg).is_err());
        cfg.user = "dev".into();
        cfg.dir = "line\nbreak".into();
        assert!(validate(&cfg).is_err());
        cfg.dir = "~/.ocr4code".into();
        cfg.server_port = 0;
        assert!(validate(&cfg).is_err());
        cfg.server_port = 8099;
        cfg.gpu_layers = 1000;
        assert!(validate(&cfg).is_err());
    }

    #[test]
    fn runtime_assets_follow_arch_and_accel() {
        let cpu = runtime_assets("x86_64", Accel::Cpu).unwrap();
        assert!(cpu[0].ends_with("/llama-b12018-bin-ubuntu-x64.tar.gz"));
        let arm = runtime_assets("aarch64", Accel::Auto).unwrap();
        assert!(arm[0].ends_with("/llama-b12018-bin-ubuntu-arm64.tar.gz"));
        let vulkan = runtime_assets("x86_64", Accel::Vulkan).unwrap();
        assert!(vulkan[0].ends_with("/llama-b12018-bin-ubuntu-vulkan-x64.tar.gz"));
        let cuda = runtime_assets("x86_64", Accel::Cuda).unwrap();
        assert_eq!(cuda.len(), 2);
        assert!(cuda[0].contains("cuda-13.4-x64"));
        assert!(runtime_assets("arm64", Accel::Cuda).is_err());
        assert!(runtime_assets("riscv64", Accel::Cpu).is_err());
    }

    #[test]
    fn server_args_bind_loopback_and_respect_the_context_plan() {
        let mut cfg = config();
        cfg.accel = Accel::Vulkan;
        cfg.gpu_layers = 42;
        let args = server_args(&cfg, "/home/dev/model.gguf", "/home/dev/mmproj.gguf", 8099, true);
        let value = |key: &str| {
            args.iter()
                .position(|arg| arg == key)
                .and_then(|index| args.get(index + 1))
                .map(String::as_str)
        };
        assert_eq!(value("--host"), Some("127.0.0.1"));
        assert_eq!(value("--port"), Some("8099"));
        assert_eq!(value("-ngl"), Some("42"));
        let slots = llama_slots(3).to_string();
        let ctx = total_ctx(3, 3072).to_string();
        assert_eq!(value("-np"), Some(slots.as_str()));
        assert_eq!(value("-c"), Some(ctx.as_str()));
        assert_eq!(value("-ctk"), Some("q8_0"));

        cfg.accel = Accel::Cpu;
        let args = server_args(&cfg, "m", "p", 8099, false);
        let ngl = args
            .iter()
            .position(|arg| arg == "-ngl")
            .and_then(|index| args.get(index + 1));
        assert_eq!(ngl.map(String::as_str), Some("0"));
    }

    #[test]
    fn probe_output_is_parsed() {
        let output = "\
OCR4CODE OS=Linux
OCR4CODE ARCH=x86_64
OCR4CODE HOME=/home/dev
OCR4CODE DIR=/home/dev/.ocr4code
OCR4CODE DISK=5000000
OCR4CODE CURL=1
OCR4CODE TAR=1
OCR4CODE SHA256=sha256sum
OCR4CODE RUNTIME=/home/dev/.ocr4code/runtime/llama-server
OCR4CODE FILE=GLM-OCR.Q4_K_M.gguf:548515136:548515136
OCR4CODE FILE=mmproj-GLM-OCR-Q8_0.gguf:484403648:484403648
OCR4CODE MARKER=xx
OCR4CODE DONE=1
";
        let probe = parse_probe(output, "xx");
        assert!(probe.reachable);
        assert_eq!(probe.os, "Linux");
        assert_eq!(probe.arch, "x86_64");
        assert_eq!(probe.dir, "/home/dev/.ocr4code");
        assert!(probe.curl);
        assert!(!probe.wget);
        assert_eq!(probe.sha256, "sha256sum");
        assert!(probe.model_installed);
        assert!(probe.model_verified);
        assert_eq!(probe.files.len(), 2);
    }

    #[test]
    fn partial_model_is_not_installed() {
        let output = "\
OCR4CODE OS=Linux
OCR4CODE FILE=GLM-OCR.Q4_K_M.gguf:100:548515136
";
        let probe = parse_probe(output, "");
        assert!(!probe.model_installed);
        assert!(!probe.files[0].present);
    }

    #[test]
    fn probe_script_expands_tilde_and_lists_both_files() {
        let script = probe_script(&config());
        assert!(script.contains("${DIR_RAW#\\~/}"));
        assert!(script.contains("GLM-OCR.Q4_K_M.gguf"));
        assert!(script.contains("mmproj-GLM-OCR-Q8_0.gguf"));
        assert!(script.contains("verified-low.json"));
    }

    #[test]
    fn start_script_quotes_paths_and_binds_loopback() {
        let cfg = config();
        let probe = Probe {
            dir: "/home/dev/.ocr4code".into(),
            runtime: Some("/home/dev/.ocr4code/runtime/llama-server".into()),
            ..Probe::default()
        };
        let script = start_script(&cfg, &probe);
        assert!(script.contains("'--host' '127.0.0.1'"));
        assert!(script.contains("nohup \"$RUNTIME\""));
        assert!(script.contains("/home/dev/.ocr4code/models/GLM-OCR.Q4_K_M.gguf"));
        assert!(script.contains("--sleep-idle-seconds"));
    }
}
