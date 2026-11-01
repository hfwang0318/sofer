//! Remote deployment tests: generated shell scripts, probe parsing and the
//! tunnel watchdog policy. Kept in one module so the submodules stay focused.

#[cfg(unix)]
use std::fs;
use std::io::{Read, Write};
use std::net::TcpListener;
#[cfg(unix)]
use std::process::{Command, Stdio};
use std::thread;
use std::time::Duration;

#[cfg(unix)]
use super::provision::upload_command;
use super::provision::{
    destination, parallel_download_script, parse_probe, probe_script, progress_command,
    runtime_assets, shell_quote,
};
use super::tunnel::{
    backoff_delay, server_args, start_script, tunnel_responds, WatchAction, WatchEvent, WatchState,
    WatchdogPolicy,
};
use crate::engine::{llama_slots, total_ctx};

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

    let root = std::env::temp_dir().join(format!("ocr4code-upload-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    let part = root.join("models").join("model name.gguf.part");
    // ssh joins its argv and lets the remote shell parse the result; this
    // `sh -c` hop reproduces that second round of parsing.
    let remote = format!(
        "sh -c {}",
        shell_quote(&upload_command(part.to_str().unwrap()))
    );
    let mut child = Command::new("sh")
        .arg("-c")
        .arg(&remote)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(b"weights").unwrap();
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
    let args = server_args(
        &cfg,
        "/home/dev/model.gguf",
        "/home/dev/mmproj.gguf",
        8099,
        true,
    );
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
    assert!(script.contains("OCR4CODE FILE=GLM-OCR.Q4_K_M.gguf:"));
    assert!(script.contains("OCR4CODE FILE=mmproj-GLM-OCR-Q8_0.gguf:"));
    assert!(!script.contains("FILE='"));
    assert!(script.contains("verified-low.json"));
}

#[test]
fn parallel_download_script_uses_ranges_and_falls_back() {
    let script = parallel_download_script(
        "https://example.com/model.gguf",
        "/home/ocr/.ocr4code/models/model.gguf",
        1000,
        4,
    );
    // 1000 bytes / 4 connections rounds up to 250-byte segments.
    assert!(script.contains("SEG=250"));
    assert!(script.contains("N=4"));
    assert!(script.contains("--range 0-0"));
    assert!(script.contains("RANGE_CODE"));
    assert!(script.contains("--range \"$FROM-$END\""));
    assert!(script.contains("part-$I"));
    assert!(script.contains("RANGE_UNSUPPORTED"));
    assert!(script.contains("fetch \"$URL\" \"$PARTS/part-fallback\""));
    assert!(script.contains("--speed-time 60"));
    assert!(script.contains("--connect-timeout 15"));
    assert!(script.contains("'https://example.com/model.gguf'"));
    assert!(script.contains("'/home/ocr/.ocr4code/models/model.gguf.parts'"));
    assert!(script.contains("'/home/ocr/.ocr4code/models/model.gguf.part'"));
}

#[cfg(unix)]
#[test]
fn parallel_download_script_parses_with_posix_sh() {
    use std::io::Write;
    use std::process::{Command, Stdio};

    let script = parallel_download_script(
        "https://example.com/model.gguf",
        "/tmp/model.gguf",
        1024 * 1024,
        4,
    );
    let mut child = Command::new("sh")
        .arg("-n")
        .stdin(Stdio::piped())
        .spawn()
        .expect("spawn sh");
    child
        .stdin
        .take()
        .unwrap()
        .write_all(script.as_bytes())
        .unwrap();
    assert!(child.wait().unwrap().success());
}

#[test]
fn progress_command_sums_finished_and_inflight_parts() {
    let command = progress_command(PollTarget::Parts("/tmp/parts"));
    assert!(command.contains("find '/tmp/parts'"));
    assert!(command.contains("-name 'part-*'"));
    assert!(!command.contains("tmp-*"));
    assert!(command.contains("awk '{s+=$1} END {print s+0}'"));

    let command = progress_command(PollTarget::File("/tmp/a.part"));
    assert_eq!(command, "stat -c %s '/tmp/a.part' 2>/dev/null || echo 0");
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

// -- tunnel watchdog ---------------------------------------------------

fn policy() -> WatchdogPolicy {
    WatchdogPolicy::default()
}

fn attempt_of(state: WatchState) -> u32 {
    match state {
        WatchState::Reconnecting { attempt } => attempt,
        other => panic!("expected reconnecting, got {other:?}"),
    }
}

#[test]
fn watchdog_policy_has_the_documented_limits() {
    let policy = policy();
    assert_eq!(policy.max_attempts, 5);
    assert_eq!(policy.max_delay, Duration::from_secs(30));
    assert_eq!(policy.failure_threshold, 2);
}

#[test]
fn healthy_probes_keep_the_tunnel_connected() {
    let mut state = WatchState::connected();
    for _ in 0..3 {
        let (next, action) = state.on_event(WatchEvent::Healthy, &policy(), 0.5);
        assert_eq!(next, WatchState::Connected { failures: 0 });
        assert_eq!(action, WatchAction::Idle);
        state = next;
    }
}

#[test]
fn failures_must_reach_the_threshold_before_reconnecting() {
    let policy = policy();
    let (next, action) = WatchState::connected().on_event(WatchEvent::Unhealthy, &policy, 0.5);
    assert_eq!(next, WatchState::Connected { failures: 1 });
    assert_eq!(action, WatchAction::Idle);

    let (next, action) = next.on_event(WatchEvent::Unhealthy, &policy, 0.0);
    assert_eq!(next, WatchState::Reconnecting { attempt: 1 });
    match action {
        WatchAction::Reconnect { attempt, delay } => {
            assert_eq!(attempt, 1);
            // 2 s base with the lowest jitter → 1.5 s.
            assert_eq!(delay, Duration::from_millis(1500));
        }
        other => panic!("expected reconnect, got {other:?}"),
    }
}

#[test]
fn a_dead_ssh_child_reconnects_without_waiting_for_the_threshold() {
    let (next, action) = WatchState::connected().on_event(WatchEvent::TunnelExited, &policy(), 0.5);
    assert_eq!(next, WatchState::Reconnecting { attempt: 1 });
    assert!(matches!(
        action,
        WatchAction::Reconnect {
            attempt: 1,
            delay: _
        }
    ));
}

#[test]
fn reconnect_attempts_back_off_and_stop_after_five() {
    let policy = policy();
    let mut state = WatchState::Reconnecting { attempt: 1 };
    let mut delays = Vec::new();
    for _ in 0..4 {
        let (next, action) = state.on_event(WatchEvent::AttemptFailed, &policy, 0.5);
        match action {
            WatchAction::Reconnect { attempt, delay } => {
                assert_eq!(attempt, attempt_of(next));
                delays.push(delay);
            }
            other => panic!("expected another attempt, got {other:?}"),
        }
        state = next;
    }
    // Attempts 2–5 with neutral jitter: 4 s, 8 s, 16 s, then 32 s capped
    // at the 30 s single-wait limit.
    assert_eq!(
        delays,
        vec![
            Duration::from_secs(4),
            Duration::from_secs(8),
            Duration::from_secs(16),
            Duration::from_secs(30),
        ]
    );

    let (next, action) = state.on_event(WatchEvent::AttemptFailed, &policy, 0.5);
    assert_eq!(next, WatchState::Failed);
    assert_eq!(action, WatchAction::Exhausted);
    // An exhausted watchdog stays failed; only an explicit restart helps.
    let (next, action) = next.on_event(WatchEvent::Unhealthy, &policy, 0.5);
    assert_eq!(next, WatchState::Failed);
    assert_eq!(action, WatchAction::Idle);
}

#[test]
fn a_successful_attempt_resets_the_failure_history() {
    let policy = policy();
    let state = WatchState::Reconnecting { attempt: 3 };
    let (next, action) = state.on_event(WatchEvent::AttemptSucceeded, &policy, 0.5);
    assert_eq!(next, WatchState::Connected { failures: 0 });
    assert_eq!(action, WatchAction::Recovered);

    // The old outage must not count toward the next one.
    let (next, _) = next.on_event(WatchEvent::Unhealthy, &policy, 0.5);
    assert_eq!(next, WatchState::Connected { failures: 1 });
}

#[test]
fn a_new_outage_restarts_the_attempt_counter() {
    let policy = policy();
    let (state, _) = WatchState::Reconnecting { attempt: 4 }.on_event(
        WatchEvent::AttemptSucceeded,
        &policy,
        0.5,
    );
    let (next, action) = state.on_event(WatchEvent::TunnelExited, &policy, 0.5);
    assert_eq!(next, WatchState::Reconnecting { attempt: 1 });
    assert!(matches!(action, WatchAction::Reconnect { attempt: 1, .. }));
}

#[test]
fn any_http_answer_proves_the_tunnel_is_alive() {
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = thread::spawn(move || {
        if let Ok((mut stream, _)) = listener.accept() {
            let mut buffer = [0u8; 1024];
            let _ = stream.read(&mut buffer);
            // 503 while the model sleeps still proves the forward works.
            let _ = stream.write_all(
                    b"HTTP/1.1 503 Service Unavailable\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                );
        }
    });
    assert!(tunnel_responds(&format!("http://127.0.0.1:{port}")));
    server.join().unwrap();
    // The listener is gone: the forward is dead, not merely unready.
    assert!(!tunnel_responds(&format!("http://127.0.0.1:{port}")));
}

#[test]
fn backoff_delay_is_jittered_and_capped_at_thirty_seconds() {
    let policy = policy();
    assert_eq!(backoff_delay(&policy, 1, 0.0), Duration::from_millis(1500));
    assert_eq!(backoff_delay(&policy, 1, 0.5), Duration::from_secs(2));
    assert_eq!(backoff_delay(&policy, 5, 0.5), Duration::from_secs(30));
    assert_eq!(backoff_delay(&policy, 5, 1.0), Duration::from_secs(30));
    for attempt in 1..=5 {
        assert!(backoff_delay(&policy, attempt, 1.0) <= policy.max_delay);
    }
    // Jitter must never invert the growth between early attempts.
    assert!(backoff_delay(&policy, 1, 1.0) < backoff_delay(&policy, 2, 0.0));
}
