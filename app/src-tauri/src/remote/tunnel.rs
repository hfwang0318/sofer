//! The `ssh -L` tunnel and its watchdog: forward the remote port, probe it,
//! reconnect with jittered backoff, and start/stop the remote server.

use std::net::TcpListener;
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use crate::engine::{health_ok, llama_slots, model_files, total_ctx};

use super::provision::{shell_quote, ssh_base, tail_text, SSH_TIMEOUT};
use super::{Accel, Error, Manager, Phase, Probe, RemoteConfig, Shared};

const LOCAL_PORT_START: u16 = 28099;
const LOCAL_PORT_END: u16 = 28_120;
const STARTUP_TIMEOUT: Duration = Duration::from_secs(240);
/// How long one reconnect attempt may take to produce a healthy tunnel.
const RECONNECT_HEALTH_TIMEOUT: Duration = Duration::from_secs(20);
/// Granularity of the watchdog's interruptible sleeps.
const WATCHDOG_POLL: Duration = Duration::from_millis(100);

// ---------------------------------------------------------------------------
// Tunnel watchdog: pure policy state machine
// ---------------------------------------------------------------------------

/// Tunables for the tunnel watchdog. The defaults are the shipped policy:
/// probe every 5 s, reconnect after two failed probes, five attempts with
/// jittered exponential backoff capped at 30 s each.
#[derive(Debug, Clone, Copy)]
pub(super) struct WatchdogPolicy {
    /// Consecutive failed health probes before a live tunnel is reconnected.
    pub(super) failure_threshold: u32,
    /// Total reconnect attempts before giving up.
    pub(super) max_attempts: u32,
    /// First backoff delay; doubles per attempt.
    pub(super) base_delay: Duration,
    /// Hard cap for a single delay.
    pub(super) max_delay: Duration,
    /// How often a live tunnel is probed.
    pub(super) check_interval: Duration,
}

impl Default for WatchdogPolicy {
    fn default() -> Self {
        Self {
            failure_threshold: 2,
            max_attempts: 5,
            base_delay: Duration::from_secs(2),
            max_delay: Duration::from_secs(30),
            check_interval: Duration::from_secs(5),
        }
    }
}

/// One observation or outcome fed into [`WatchState`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum WatchEvent {
    /// The forwarded port answered `/health`.
    Healthy,
    /// One probe failed but the ssh child is still alive.
    Unhealthy,
    /// The ssh child exited; reconnect immediately, skipping the threshold.
    TunnelExited,
    /// A reconnect attempt did not produce a healthy tunnel.
    AttemptFailed,
    /// A reconnect attempt produced a healthy tunnel.
    AttemptSucceeded,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum WatchState {
    Connected { failures: u32 },
    Reconnecting { attempt: u32 },
    Failed,
}

impl WatchState {
    pub(super) fn connected() -> Self {
        Self::Connected { failures: 0 }
    }

    /// The whole reconnect policy lives here: no I/O, so tests drive every
    /// branch without a real ssh process or server.
    pub(super) fn on_event(
        self,
        event: WatchEvent,
        policy: &WatchdogPolicy,
        jitter: f64,
    ) -> (Self, WatchAction) {
        match self {
            WatchState::Connected { failures } => match event {
                WatchEvent::Healthy => (Self::connected(), WatchAction::Idle),
                WatchEvent::Unhealthy => {
                    let failures = failures + 1;
                    if failures >= policy.failure_threshold {
                        Self::retry(1, policy, jitter)
                    } else {
                        (WatchState::Connected { failures }, WatchAction::Idle)
                    }
                }
                WatchEvent::TunnelExited => Self::retry(1, policy, jitter),
                WatchEvent::AttemptSucceeded => (Self::connected(), WatchAction::Idle),
                WatchEvent::AttemptFailed => {
                    (WatchState::Connected { failures }, WatchAction::Idle)
                }
            },
            WatchState::Reconnecting { attempt } => match event {
                WatchEvent::AttemptSucceeded => (Self::connected(), WatchAction::Recovered),
                WatchEvent::AttemptFailed | WatchEvent::TunnelExited | WatchEvent::Unhealthy => {
                    let next = attempt + 1;
                    if next > policy.max_attempts {
                        (WatchState::Failed, WatchAction::Exhausted)
                    } else {
                        Self::retry(next, policy, jitter)
                    }
                }
                WatchEvent::Healthy => (Self::connected(), WatchAction::Idle),
            },
            WatchState::Failed => (WatchState::Failed, WatchAction::Idle),
        }
    }

    fn retry(attempt: u32, policy: &WatchdogPolicy, jitter: f64) -> (Self, WatchAction) {
        (
            WatchState::Reconnecting { attempt },
            WatchAction::Reconnect {
                attempt,
                delay: backoff_delay(policy, attempt, jitter),
            },
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum WatchAction {
    /// Nothing to do; keep probing.
    Idle,
    /// Publish `reconnecting`, wait for `delay`, then run one attempt.
    Reconnect { attempt: u32, delay: Duration },
    /// The tunnel is healthy again; publish `online`.
    Recovered,
    /// Every attempt failed; publish `failed` with a next step.
    Exhausted,
}

/// Jittered exponential backoff. `jitter` is a unit value in `[0, 1]`; tests
/// pass a fixed value so the delay is deterministic.
pub(super) fn backoff_delay(policy: &WatchdogPolicy, attempt: u32, jitter: f64) -> Duration {
    let exponent = attempt.saturating_sub(1).min(31);
    let factor = 1u32 << exponent;
    let base = policy
        .base_delay
        .saturating_mul(factor)
        .min(policy.max_delay);
    // ±25% around the exponential step, then clamp so one attempt never waits
    // longer than max_delay.
    let scaled = base.mul_f64(0.75 + 0.5 * jitter.clamp(0.0, 1.0));
    scaled.min(policy.max_delay).max(Duration::from_millis(1))
}

/// A fresh random value for the backoff jitter, without adding a dependency.
fn jitter_unit() -> f64 {
    use std::collections::hash_map::RandomState;
    use std::hash::{BuildHasher, Hasher};

    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_nanos() as u64)
        .unwrap_or(0);
    let mut hasher = RandomState::new().build_hasher();
    hasher.write_u64(nanos);
    (hasher.finish() % 1000) as f64 / 1000.0
}

impl Shared {
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

    pub(super) fn stop_tunnel(&self) {
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

    // -- watchdog ----------------------------------------------------------

    fn observe_tunnel(&self) -> WatchEvent {
        if let Some(mut child) = self.tunnel_exited() {
            let _ = child.wait();
            return WatchEvent::TunnelExited;
        }
        let url = self.slot.lock().unwrap().url.clone();
        match url {
            // No URL means the tunnel was torn down underneath us.
            None => WatchEvent::TunnelExited,
            Some(url) if tunnel_responds(&url) => WatchEvent::Healthy,
            Some(_) => WatchEvent::Unhealthy,
        }
    }

    /// One reconnect attempt: replace the ssh child and wait for the forwarded
    /// port to answer again. The remote `llama-server` is left untouched.
    fn reconnect_once(&self, cancel: &AtomicBool, stop: &AtomicBool, cfg: &RemoteConfig) -> bool {
        let _lifecycle = self.lifecycle.lock().unwrap();
        if !watch_should_run(cancel, stop) {
            return false;
        }
        let url = match self.start_tunnel(cfg) {
            Ok(url) => url,
            Err(_) => return false,
        };
        let deadline = Instant::now() + RECONNECT_HEALTH_TIMEOUT;
        while Instant::now() < deadline {
            if !watch_should_run(cancel, stop) {
                self.stop_tunnel();
                return false;
            }
            if tunnel_responds(&url) {
                let mut slot = self.slot.lock().unwrap();
                slot.url = Some(url);
                slot.remote_url = Some(format!("http://127.0.0.1:{}", cfg.server_port));
                return true;
            }
            thread::sleep(Duration::from_millis(300));
        }
        self.stop_tunnel();
        false
    }

    fn mark_reconnecting(&self, attempt: u32, limit: u32) {
        {
            let mut slot = self.slot.lock().unwrap();
            slot.phase = Phase::Reconnecting;
            slot.error = None;
            slot.progress = None;
            slot.reconnect_attempt = Some(attempt);
            slot.reconnect_limit = Some(limit);
        }
        self.emit_state();
    }

    fn mark_online(&self) {
        {
            let mut slot = self.slot.lock().unwrap();
            slot.phase = Phase::Online;
            slot.error = None;
            slot.reconnect_attempt = None;
            slot.reconnect_limit = None;
        }
        self.emit_state();
    }

    fn mark_reconnect_failed(&self, limit: u32) {
        {
            let mut slot = self.slot.lock().unwrap();
            slot.phase = Phase::Failed;
            slot.error = Some(format!(
                "和服务器断开了，自动重连 {limit} 次没有成功。确认服务器在线、网络正常，然后点「重连」。"
            ));
            slot.progress = None;
            slot.reconnect_attempt = None;
            slot.reconnect_limit = None;
        }
        self.emit_state();
    }
}

/// Whether the tunnel still forwards traffic. Any HTTP answer counts, including
/// 503 while `llama-server` sleeps after its idle timeout: a sleeping server is
/// not a broken tunnel, and the next capture wakes it. Only a transport error
/// means the forward is gone.
pub(super) fn tunnel_responds(url: &str) -> bool {
    if url.trim().is_empty() {
        return false;
    }
    match ureq::get(&format!("{}/health", url.trim_end_matches('/')))
        .timeout(Duration::from_millis(1500))
        .call()
    {
        Ok(_) => true,
        Err(ureq::Error::Status(_, _)) => true,
        Err(_) => false,
    }
}

/// Whether the watchdog should keep touching the tunnel. Deliberately not
/// gated on `enabled`: the flag can still be false right after autostart, and
/// disabling remote mode stops the watchdog explicitly.
fn watch_should_run(cancel: &AtomicBool, stop: &AtomicBool) -> bool {
    !stop.load(Ordering::SeqCst) && !cancel.load(Ordering::SeqCst)
}

/// Sleep in small steps so a stop request is honored within `WATCHDOG_POLL`.
fn sleep_unless_stopped(stop: &AtomicBool, duration: Duration) -> bool {
    let deadline = Instant::now() + duration;
    loop {
        if stop.load(Ordering::SeqCst) {
            return false;
        }
        let now = Instant::now();
        if now >= deadline {
            return true;
        }
        thread::sleep((deadline - now).min(WATCHDOG_POLL));
    }
}

/// Background tunnel watchdog. Probes a live tunnel, then reconnects with
/// jittered exponential backoff when the ssh child exits or health checks keep
/// failing. It exits on stop/cancel/disable or after exhausting its attempts.
fn watchdog_loop(
    shared: Arc<Shared>,
    cancel: Arc<AtomicBool>,
    stop: Arc<AtomicBool>,
    cfg: RemoteConfig,
) {
    let policy = WatchdogPolicy::default();
    let mut state = WatchState::connected();

    'watch: loop {
        // Connected: probe until something breaks.
        loop {
            if !sleep_unless_stopped(&stop, policy.check_interval) {
                return;
            }
            if !watch_should_run(&cancel, &stop) {
                return;
            }
            let event = shared.observe_tunnel();
            let (next, action) = state.on_event(event, &policy, jitter_unit());
            state = next;
            if !matches!(action, WatchAction::Idle) {
                break;
            }
        }

        // Reconnecting: publish the phase, then back off between attempts.
        loop {
            let WatchState::Reconnecting { attempt } = state else {
                return;
            };
            shared.mark_reconnecting(attempt, policy.max_attempts);
            let delay = backoff_delay(&policy, attempt, jitter_unit());
            if !sleep_unless_stopped(&stop, delay) {
                return;
            }
            if !watch_should_run(&cancel, &stop) {
                return;
            }

            let ok = shared.reconnect_once(&cancel, &stop, &cfg);
            let event = if ok {
                WatchEvent::AttemptSucceeded
            } else {
                WatchEvent::AttemptFailed
            };
            let (next, action) = state.on_event(event, &policy, jitter_unit());
            state = next;
            match action {
                WatchAction::Recovered => {
                    if watch_should_run(&cancel, &stop) {
                        shared.mark_online();
                    }
                    continue 'watch;
                }
                WatchAction::Reconnect { .. } => {}
                WatchAction::Exhausted => {
                    if watch_should_run(&cancel, &stop) {
                        shared.mark_reconnect_failed(policy.max_attempts);
                    }
                    return;
                }
                WatchAction::Idle => return,
            }
        }
    }
}

impl Manager {
    pub(super) fn stop_watchdog(&self) {
        if let Some(stop) = self.watchdog.lock().unwrap().take() {
            stop.store(true, Ordering::SeqCst);
        }
    }

    /// Start watching a tunnel that just came up. Any previous watchdog is
    /// stopped first so it cannot race the fresh one.
    fn spawn_watchdog(&self, cfg: &RemoteConfig) {
        self.stop_watchdog();
        let stop = Arc::new(AtomicBool::new(false));
        *self.watchdog.lock().unwrap() = Some(stop.clone());
        let shared = self.shared.clone();
        let cancel = self.cancel.clone();
        let cfg = cfg.clone();
        thread::spawn(move || watchdog_loop(shared, cancel, stop, cfg));
    }

    pub(super) fn start_inner(&self, cfg: &RemoteConfig, probe: &Probe) -> Result<String, Error> {
        let url = self.start_tunnel(cfg)?;
        let _ = probe;
        // A server started by an earlier app session can still be healthy; in
        // that case skip the process restart and reuse it.
        let healthy = self.remote_healthy(cfg).unwrap_or(false);
        if !healthy {
            self.progress("starting", Some("正在启动 OCR 引擎".into()), None, 0, 0);
            let script = start_script(cfg, probe);
            let output = self.run_script(cfg, &script, SSH_TIMEOUT, None)?;
            if output.contains("START_ERROR") {
                self.stop_tunnel();
                return Err(Error::Remote(format!(
                    "OCR 引擎没有启动起来。\n{}",
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
                self.progress("ready", Some("OCR 引擎已就绪".into()), None, 0, 0);
                self.spawn_watchdog(cfg);
                return Ok(url);
            }
            thread::sleep(Duration::from_millis(400));
        }
        self.stop_tunnel();
        Err(Error::Timeout(
            "OCR 引擎在 240 秒内没有就绪。查看服务器日志后重试。".into(),
        ))
    }
}

pub(super) fn start_script(cfg: &RemoteConfig, probe: &Probe) -> String {
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
         if [ ! -x \"$RUNTIME\" ] && ! command -v \"$RUNTIME\" >/dev/null 2>&1; then echo SOFER START_ERROR=1; exit 4; fi\n\
         nohup \"$RUNTIME\" {args} </dev/null >>\"$LOG\" 2>&1 &\n\
         NEW=$!\n\
         echo \"$NEW\" > \"$PIDFILE\"\n\
         sleep 0.6\n\
         if ! kill -0 \"$NEW\" 2>/dev/null; then\n\
           echo SOFER START_ERROR=1\n\
           tail -n 20 \"$LOG\" 2>/dev/null\n\
           exit 4\n\
         fi\n\
         echo \"SOFER PID=$NEW\"\n",
        args = quoted_args
    ));
    script
}

pub(super) fn stop_script(cfg: &RemoteConfig) -> String {
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
         echo SOFER STOPPED=1\n",
    );
    script
}

pub(super) fn server_args(
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
