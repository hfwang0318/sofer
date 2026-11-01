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
//!
//! The work is split by concern: [`provision`] runs the SSH scripts that probe,
//! deploy and transfer the pack; [`tunnel`] owns the forwarded port and its
//! watchdog. This module holds the state machine and the public API.

use std::io::{Read, Write};
use std::path::PathBuf;
use std::process::{Child, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::engine::{health_ok, ModelQuality};

pub use self::provision::validate;
use self::provision::{progress_command, ssh_base, tail_text};
use self::tunnel::stop_script;

mod provision;
mod tunnel;

#[cfg(test)]
mod tests;

/// What `run_script` should poll for download progress.
#[derive(Debug, Clone, Copy)]
enum PollTarget<'a> {
    /// A single file, e.g. `model.gguf.part`.
    File(&'a str),
    /// A directory of `part-*` files used by a ranged download.
    Parts(&'a str),
}

/// Where the weights come from when the server does not have them yet.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Source {
    /// Use whatever is already there; otherwise download on the server.
    #[default]
    Auto,
    /// Always download on the server.
    Download,
    /// Stream the verified local copy over SSH.
    Upload,
}

/// Which llama.cpp Linux build to install when the server has none.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Accel {
    #[default]
    Auto,
    Cpu,
    Vulkan,
    Cuda,
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
    /// stopped | probing | deploying | starting | reconnecting | online | failed | cancelled
    pub phase: String,
    pub url: Option<String>,
    pub remote_url: Option<String>,
    pub tunnel: bool,
    pub error: Option<String>,
    pub quality: Option<ModelQuality>,
    pub deployed: bool,
    pub progress: Option<Progress>,
    /// 1-based attempt while the watchdog is reconnecting.
    pub reconnect_attempt: Option<u32>,
    /// Total attempts the watchdog will make before giving up.
    pub reconnect_limit: Option<u32>,
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
    /// The watchdog is restarting the tunnel after it broke.
    Reconnecting,
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
            Phase::Reconnecting => "reconnecting",
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
    reconnect_attempt: Option<u32>,
    reconnect_limit: Option<u32>,
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
            reconnect_attempt: None,
            reconnect_limit: None,
        }
    }
}

/// State the tunnel watchdog also needs. Kept behind an `Arc` so the watchdog
/// thread can observe and repair the tunnel without owning the manager.
///
/// Public only because it is `Deref`'s target for `Manager`; the fields stay
/// private to this module.
pub struct Shared {
    sink: EventSink,
    slot: Mutex<Slot>,
    /// One deploy/start/stop/reconnect at a time.
    lifecycle: Mutex<()>,
    enabled: AtomicBool,
}

pub struct Manager {
    shared: Arc<Shared>,
    /// Where the desktop app keeps its verified local copy (for uploads).
    local_models: PathBuf,
    /// Last config used, so shutdown can release the remote process.
    last_config: Mutex<Option<RemoteConfig>>,
    pub cancel: Arc<AtomicBool>,
    pub deploy_running: AtomicBool,
    /// Stop flag of the running watchdog, if any.
    watchdog: Mutex<Option<Arc<AtomicBool>>>,
}

impl std::ops::Deref for Manager {
    type Target = Shared;

    /// Field and method access falls through to the shared state, so the
    /// existing `self.slot` / `self.emit_state()` call sites stay unchanged.
    fn deref(&self) -> &Shared {
        &self.shared
    }
}

impl Shared {
    fn state(&self) -> State {
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
            reconnect_attempt: slot.reconnect_attempt,
            reconnect_limit: slot.reconnect_limit,
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
            if !matches!(phase, Phase::Reconnecting) {
                slot.reconnect_attempt = None;
                slot.reconnect_limit = None;
            }
        }
        self.emit_state();
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
}

impl Manager {
    pub fn new(local_models: PathBuf, sink: EventSink) -> Self {
        Self {
            shared: Arc::new(Shared {
                sink,
                slot: Mutex::new(Slot::default()),
                lifecycle: Mutex::new(()),
                enabled: AtomicBool::new(false),
            }),
            local_models,
            last_config: Mutex::new(None),
            cancel: Arc::new(AtomicBool::new(false)),
            deploy_running: AtomicBool::new(false),
            watchdog: Mutex::new(None),
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
        Shared::state(self)
    }

    /// Inspect the target without changing anything. Safe to call repeatedly.
    /// Serialized with deploy/start so a background probe cannot overwrite the
    /// phase of an in-flight deployment.
    pub fn probe(&self, cfg: &RemoteConfig) -> Result<Probe, Error> {
        validate(cfg)?;
        // A manual probe owns the phase; a watchdog tick in the middle of it
        // would rewrite it.
        self.stop_watchdog();
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

    /// One-click deploy. Runs on a worker thread; progress goes to the sink.
    pub fn deploy(&self, cfg: &RemoteConfig, source: Source) -> Result<(), Error> {
        validate(cfg)?;
        // Stop the watchdog before waiting for the lifecycle lock so an
        // in-flight reconnect cannot hold up a redeploy.
        self.stop_watchdog();
        let _lifecycle = self.lifecycle.lock().unwrap();
        // shutdown() needs the config to release a server started by deploy.
        self.remember(cfg);
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

    /// Reuse a healthy tunnel, otherwise start the remote server and the tunnel.
    pub fn ensure_ready(&self, cfg: &RemoteConfig) -> Result<String, Error> {
        validate(cfg)?;
        self.remember(cfg);
        if let Some(url) = self.online_url() {
            return Ok(url);
        }
        // Reconnecting never wins over an explicit start: stop the watchdog
        // first so a pending attempt aborts instead of holding the lock.
        self.stop_watchdog();
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
                "服务器上还没有 OCR 模型。到设置 → OCR 中完成部署。".into(),
            ));
        }
        if probe.runtime.is_none() {
            return Err(Error::NotDeployed(
                "服务器上还没有 OCR 运行时。到设置 → OCR 中重新部署。".into(),
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

    /// Stop the tunnel and the remote process. Best effort: a stop must not
    /// block quitting the app for long.
    pub fn stop(&self) {
        // Signal an in-flight download/start before waiting for the lifecycle
        // lock, otherwise a long ensure_ready can block Settings for minutes.
        self.cancel.store(true, Ordering::SeqCst);
        self.stop_watchdog();
        let _lifecycle = self.lifecycle.lock().unwrap();
        self.stop_tunnel();
        self.set_phase(Phase::Stopped, None);
    }

    /// Stop with an explicit config so the remote process is released too.
    pub fn stop_with(&self, cfg: &RemoteConfig) {
        self.cancel.store(true, Ordering::SeqCst);
        self.stop_watchdog();
        let _lifecycle = self.lifecycle.lock().unwrap();
        self.remember(cfg);
        self.stop_tunnel();
        if validate(cfg).is_ok() {
            let script = stop_script(cfg);
            // The cancel flag is set above to abort any in-flight deploy; it
            // must not abort the stop script itself.
            let _ = self.run_script_inner(cfg, &script, Duration::from_secs(12), None, false);
        }
        self.set_phase(Phase::Stopped, None);
    }

    pub fn shutdown(&self) {
        self.cancel.store(true, Ordering::SeqCst);
        self.stop_watchdog();
        self.stop_tunnel();
        let cfg = self.last_config.lock().unwrap().clone();
        if let Some(cfg) = cfg {
            if validate(&cfg).is_ok() {
                let script = stop_script(&cfg);
                // Quitting must still release the remote process after cancel
                // was raised for an in-flight deploy.
                let _ = self.run_script_inner(&cfg, &script, Duration::from_secs(5), None, false);
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

    fn progress_bytes(
        &self,
        phase: &str,
        file: &str,
        downloaded: u64,
        total: u64,
        started: Instant,
    ) {
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

    fn run_script(
        &self,
        cfg: &RemoteConfig,
        script: &str,
        timeout: Duration,
        poll: Option<(PollTarget<'_>, &str, u64)>,
    ) -> Result<String, Error> {
        self.run_script_inner(cfg, script, timeout, poll, true)
    }

    /// `cancelable = false` is used by the stop scripts: `stop_with` and
    /// `shutdown` raise `cancel` first, and a cancelled stop would leave
    /// `llama-server` running on the server.
    fn run_script_inner(
        &self,
        cfg: &RemoteConfig,
        script: &str,
        timeout: Duration,
        poll: Option<(PollTarget<'_>, &str, u64)>,
        cancelable: bool,
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
            if cancelable && self.cancel.load(Ordering::SeqCst) {
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
            if let Some((target, file, total)) = poll {
                if last_poll.elapsed() >= Duration::from_secs(2) {
                    let bytes = self.remote_progress_bytes(cfg, target).unwrap_or(0);
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
            // Scripts such as start_script put the interesting log tail on
            // stdout (and exit non-zero), so keep it instead of reporting only
            // the sometimes-empty stderr.
            let mut detail = String::from_utf8_lossy(&err).trim().to_string();
            let stdout_tail = tail_text(&String::from_utf8_lossy(&out));
            if !stdout_tail.trim().is_empty() {
                if !detail.is_empty() {
                    detail.push('\n');
                }
                detail.push_str(stdout_tail.trim());
            }
            return Err(Error::Remote(if detail.is_empty() {
                "服务器命令执行失败。检查服务器环境后重试。".into()
            } else {
                format!("服务器命令执行失败：{detail}")
            }));
        }
        Ok(String::from_utf8_lossy(&out).to_string())
    }

    fn remote_progress_bytes(
        &self,
        cfg: &RemoteConfig,
        target: PollTarget<'_>,
    ) -> Result<u64, Error> {
        let mut cmd = ssh_base(cfg)?;
        cmd.arg(progress_command(target))
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
        self.stop_watchdog();
        if let Ok(mut slot) = self.slot.lock() {
            if let Some(mut child) = slot.tunnel.take() {
                let _ = child.kill();
                let _ = child.wait();
            }
        }
    }
}
