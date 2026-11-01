//! SSH provisioning: probe the target, install the pinned runtime, fetch or
//! upload the weights, verify them and run deployment scripts.

use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use crate::engine::{model_files, model_url, pack_manifest};

use super::{Accel, Error, Manager, PollTarget, Probe, ProbeFile, RemoteConfig, Source};

const RUNTIME_TAG: &str = "b12018";
const RUNTIME_BASE: &str = "https://github.com/ggml-org/llama.cpp/releases/download";
pub(super) const SSH_TIMEOUT: Duration = Duration::from_secs(30);
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(4 * 60 * 60);
/// Files at least this large use ranged, multi-connection downloads.
const PARALLEL_MIN_BYTES: u64 = 16 * 1024 * 1024;
/// Range requests run in parallel; 4 is a good balance between speed and the
/// server/CDN tolerating the connections.
const DOWNLOAD_CONNECTIONS: u32 = 4;

impl Manager {
    pub(super) fn probe_inner(&self, cfg: &RemoteConfig) -> Result<Probe, Error> {
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
                if probe.os.is_empty() {
                    "未知系统"
                } else {
                    &probe.os
                }
            )));
        }
        Ok(probe)
    }

    pub(super) fn deploy_inner(&self, cfg: &RemoteConfig, source: Source) -> Result<(), Error> {
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
        self.progress("runtime", Some("正在安装 OCR 运行时".into()), None, 0, 0);
        let mut script = String::from("set -u\n");
        script.push_str(&format!("DIR={}\n", shell_quote(&probe.dir)));
        script.push_str("mkdir -p \"$DIR/runtime\" \"$DIR/runtime.tmp\"\n");
        script.push_str("rm -rf \"$DIR/runtime.tmp\"/*\n");
        for (index, url) in assets.iter().enumerate() {
            let archive = format!("$DIR/runtime-{index}.tar.gz");
            script.push_str(&download_function());
            script.push_str(&format!(
                "fetch {} {} || {{ echo SOFER FETCH_FAILED=1; exit 3; }}\n",
                shell_quote(url),
                archive
            ));
            script.push_str(&format!(
                "tar -xzf {} -C \"$DIR/runtime.tmp\" || {{ echo SOFER UNTAR_FAILED=1; exit 3; }}\n",
                archive
            ));
        }
        script.push_str(
            "find \"$DIR/runtime.tmp\" \\( -type f -o -type l \\) -exec cp -a {} \"$DIR/runtime/\" \\; 2>/dev/null\n\
             chmod +x \"$DIR/runtime/llama-server\" 2>/dev/null\n\
             rm -rf \"$DIR/runtime.tmp\" \"$DIR/runtime-\"*.tar.gz\n\
             if [ ! -x \"$DIR/runtime/llama-server\" ]; then echo SOFER RUNTIME_MISSING=1; exit 4; fi\n\
             echo SOFER RUNTIME_OK=1\n",
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
            let parts = format!("{target}.parts");
            let parallel = probe.curl && file.bytes >= PARALLEL_MIN_BYTES;
            self.progress_bytes("downloading", file.name, 0, file.bytes, Instant::now());
            let script = if parallel {
                parallel_download_script(&url, &target, file.bytes, DOWNLOAD_CONNECTIONS)
            } else {
                format!(
                    "set -u\nDIR={dir}\nmkdir -p \"$DIR/models\"\n{download}\n\
                     fetch {url} {part} || {{ echo SOFER FETCH_FAILED=1; exit 3; }}\n\
                     mv -f {part} {target}\n\
                     echo SOFER FILE_OK=1\n",
                    dir = shell_quote(&probe.dir),
                    download = download_function(),
                    url = shell_quote(&url),
                    part = shell_quote(&part),
                    target = shell_quote(&target),
                )
            };
            let poll = if parallel {
                PollTarget::Parts(parts.as_str())
            } else {
                PollTarget::File(part.as_str())
            };
            let started = Instant::now();
            let output = self.run_script(
                cfg,
                &script,
                DOWNLOAD_TIMEOUT,
                Some((poll, file.name, file.bytes)),
            )?;
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
            "set -e\nmv -f {} {}\necho SOFER FILE_OK=1\n",
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
        let mut script = format!("cd {}\n", shell_quote(&format!("{}/models", probe.dir)));
        for file in model_files(cfg.quality) {
            let name = file.name;
            let quoted = shell_quote(name);
            script.push_str(&format!(
                "if [ -f {quoted} ]; then echo \"SOFER HASH={name}:$({tool} {quoted} | awk '{{print $1}}')\"; else echo \"SOFER MISSING={name}\"; fi\n",
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
            "printf '%s' {} > {}\necho SOFER VERIFIED=1\n",
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
        let script = format!("set -e\nrm -f {}\necho SOFER REMOVED=1\n", paths.join(" "));
        self.run_script(cfg, &script, SSH_TIMEOUT, None)?;
        Ok(())
    }

    pub(super) fn remote_healthy(&self, cfg: &RemoteConfig) -> Result<bool, Error> {
        let script = format!(
            "if command -v curl >/dev/null 2>&1; then curl -fsS --max-time 2 http://127.0.0.1:{}/health >/dev/null 2>&1 && echo SOFER HEALTHY=1; else exit 7; fi\n",
            cfg.server_port
        );
        let output = self.run_script(cfg, &script, Duration::from_secs(12), None)?;
        Ok(output.contains("HEALTHY=1"))
    }
}

// ---------------------------------------------------------------------------
// Scripts and parsing
// ---------------------------------------------------------------------------

pub(super) fn shell_quote(value: &str) -> String {
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
pub(super) fn upload_command(part: &str) -> String {
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

pub(super) fn destination(cfg: &RemoteConfig) -> String {
    format!("{}@{}", cfg.user, cfg.host)
}

pub(super) fn ssh_base(cfg: &RemoteConfig) -> Result<Command, Error> {
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

pub(super) fn probe_script(cfg: &RemoteConfig) -> String {
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
         echo \"SOFER OS=$(uname -s 2>/dev/null)\"\n\
         echo \"SOFER ARCH=$(uname -m 2>/dev/null)\"\n\
         echo \"SOFER HOME=$HOME\"\n\
         echo \"SOFER DIR=$DIR\"\n\
         FREE=$(df -Pk \"$DIR\" 2>/dev/null | awk 'NR==2 {print $4}')\n\
         [ -n \"${FREE:-}\" ] && echo \"SOFER DISK=$((FREE * 1024))\"\n\
         command -v curl >/dev/null 2>&1 && echo \"SOFER CURL=1\"\n\
         command -v wget >/dev/null 2>&1 && echo \"SOFER WGET=1\"\n\
         command -v tar >/dev/null 2>&1 && echo \"SOFER TAR=1\"\n\
         if command -v sha256sum >/dev/null 2>&1; then echo \"SOFER SHA256=sha256sum\";\n\
         elif command -v shasum >/dev/null 2>&1; then echo \"SOFER SHA256=shasum\"; fi\n\
         if [ -x \"$DIR/runtime/llama-server\" ]; then echo \"SOFER RUNTIME=$DIR/runtime/llama-server\";\n\
         elif command -v llama-server >/dev/null 2>&1; then echo \"SOFER RUNTIME=$(command -v llama-server)\"; fi\n",
    );
    for file in model_files(cfg.quality) {
        // The pinned names contain only [A-Za-z0-9._-]; emit the bare name so
        // parse_probe sees the same string as model_files(). Quoting here used
        // to leak the quotes into ProbeFile::name and made every deployment
        // look like the model was missing.
        script.push_str(&format!(
            "if [ -f \"$DIR/models/{name}\" ]; then GOT=$(stat -c %s \"$DIR/models/{name}\" 2>/dev/null || echo 0); else GOT=0; fi\n\
             echo \"SOFER FILE={name}:$GOT:{bytes}\"\n",
            name = file.name,
            bytes = file.bytes
        ));
    }
    script.push_str(&format!(
        "MARKER=\"$DIR/models/verified-{}.json\"\n\
         if [ -f \"$MARKER\" ]; then echo \"SOFER MARKER=$(cat \"$MARKER\" 2>/dev/null | tr -d '\\n')\"; fi\n\
         echo SOFER DONE=1\n",
        cfg.quality.as_str()
    ));
    script
}

/// Multi-connection ranged download for one large file.
///
/// Each worker appends complete range chunks to `part-N`, so a cancelled or
/// interrupted run resumes from the bytes already on disk. If the server
/// ignores `Range` (returns the whole file), the script falls back to the
/// single-connection `fetch` used by the sequential path.
pub(super) fn parallel_download_script(
    url: &str,
    target: &str,
    total: u64,
    connections: u32,
) -> String {
    // Never create an empty segment: a model file is always large, but keep
    // the invariant so the assembly loop can require an exact size.
    let n = (connections.max(1) as u64).min(total.max(1));
    let segment = total.div_ceil(n);
    let part = format!("{target}.part");
    let parts = format!("{target}.parts");
    let template = r#"set -u
URL=__URL__
TARGET=__TARGET__
PART=__PART__
PARTS=__PARTS__
TOTAL=__TOTAL__
SEG=__SEG__
N=__N__
mkdir -p "$(dirname "$TARGET")" "$PARTS"
PIDS=""
trap 'for p in $PIDS; do kill "$p" 2>/dev/null; done' EXIT HUP INT TERM
__DOWNLOAD__
RANGE_CODE=$(curl -fsL --connect-timeout 15 --max-time 30 --range 0-0 -o /dev/null -w '%{http_code}' "$URL" 2>/dev/null || echo 000)
if [ "$RANGE_CODE" != "206" ]; then
  echo SOFER RANGE_UNSUPPORTED=1
  fetch "$URL" "$PARTS/part-fallback" || { echo SOFER FETCH_FAILED=1; exit 3; }
  mv -f "$PARTS/part-fallback" "$TARGET"
  rm -rf "$PARTS"
  echo SOFER FILE_OK=1
  exit 0
fi
segment() {
  I=$1
  START=$((I * SEG))
  END=$((START + SEG - 1))
  if [ "$START" -ge "$TOTAL" ]; then return 0; fi
  if [ "$END" -ge "$TOTAL" ]; then END=$((TOTAL - 1)); fi
  WANT=$((END - START + 1))
  PARTFILE="$PARTS/part-$I"
  HAVE=0
  if [ -f "$PARTFILE" ]; then HAVE=$(stat -c %s "$PARTFILE" 2>/dev/null || echo 0); fi
  if [ "$HAVE" -gt "$WANT" ]; then HAVE=0; rm -f "$PARTFILE"; fi
  TRIES=0
  while [ "$HAVE" -lt "$WANT" ]; do
    FROM=$((START + HAVE))
    # No curl --retry here: a retried response would be appended after the
    # partial bytes from the failed attempt. The outer loop resumes from the
    # file size instead, which also works after a cancel/restart.
    curl -fL --connect-timeout 15 --speed-time 60 --speed-limit 1024 \
      --no-progress-meter --range "$FROM-$END" "$URL" >> "$PARTFILE"
    NEW=$(stat -c %s "$PARTFILE" 2>/dev/null || echo 0)
    if [ "$NEW" -gt "$WANT" ]; then rm -f "$PARTFILE"; return 2; fi
    if [ "$NEW" -le "$HAVE" ]; then
      TRIES=$((TRIES + 1))
      if [ "$TRIES" -ge 3 ]; then return 1; fi
    else
      TRIES=0
    fi
    HAVE=$NEW
  done
  return 0
}
I=0
while [ "$I" -lt "$N" ]; do
  segment "$I" &
  PIDS="$PIDS $!"
  I=$((I + 1))
done
FAIL=0
RANGE_UNSUPPORTED=0
for P in $PIDS; do
  wait "$P"
  RC=$?
  if [ "$RC" -eq 2 ]; then RANGE_UNSUPPORTED=1; elif [ "$RC" -ne 0 ]; then FAIL=1; fi
done
PIDS=""
if [ "$RANGE_UNSUPPORTED" -eq 1 ]; then
  rm -rf "$PARTS"
  mkdir -p "$PARTS"
  echo SOFER RANGE_UNSUPPORTED=1
  fetch "$URL" "$PARTS/part-fallback" || { echo SOFER FETCH_FAILED=1; exit 3; }
  mv -f "$PARTS/part-fallback" "$TARGET"
  rm -rf "$PARTS"
  echo SOFER FILE_OK=1
  exit 0
fi
if [ "$FAIL" -ne 0 ]; then
  echo SOFER FETCH_FAILED=1
  exit 3
fi
I=0
while [ "$I" -lt "$N" ]; do
  START=$((I * SEG)); END=$((START + SEG - 1))
  if [ "$END" -ge "$TOTAL" ]; then END=$((TOTAL - 1)); fi
  WANT=$((END - START + 1))
  HAVE=$(stat -c %s "$PARTS/part-$I" 2>/dev/null || echo 0)
  if [ "$HAVE" -ne "$WANT" ]; then echo SOFER FETCH_FAILED=1; exit 3; fi
  I=$((I + 1))
done
: > "$PART"
I=0
while [ "$I" -lt "$N" ]; do cat "$PARTS/part-$I" >> "$PART"; I=$((I + 1)); done
mv -f "$PART" "$TARGET"
rm -rf "$PARTS"
echo SOFER FILE_OK=1
"#;
    template
        .replace("__URL__", &shell_quote(url))
        .replace("__TARGET__", &shell_quote(target))
        .replace("__PART__", &shell_quote(&part))
        .replace("__PARTS__", &shell_quote(&parts))
        .replace("__TOTAL__", &total.to_string())
        .replace("__SEG__", &segment.to_string())
        .replace("__N__", &n.to_string())
        .replace("__DOWNLOAD__", &download_function())
}

pub(super) fn progress_command(target: PollTarget<'_>) -> String {
    match target {
        PollTarget::File(path) => format!(
            "stat -c %s {} 2>/dev/null || echo 0",
            shell_quote(path)
        ),
        PollTarget::Parts(dir) => format!(
            "find {} -type f -name 'part-*' -exec stat -c %s {{}} + 2>/dev/null | awk '{{s+=$1}} END {{print s+0}}'",
            shell_quote(dir)
        ),
    }
}

fn download_function() -> String {
    "fetch() {\n\
       if command -v curl >/dev/null 2>&1; then\n\
         curl -fL --retry 3 --connect-timeout 15 --speed-time 60 --speed-limit 1024 -C - --no-progress-meter -o \"$2\" \"$1\" || \\\n\
           curl -fL --retry 3 --connect-timeout 15 --speed-time 60 --speed-limit 1024 --no-progress-meter -o \"$2\" \"$1\";\n\
       elif command -v wget >/dev/null 2>&1; then\n\
         wget -q -c --timeout=30 --tries=3 -O \"$2\" \"$1\";\n\
       else\n\
         return 1;\n\
       fi\n\
     }\n"
        .to_string()
}

pub(super) fn runtime_assets(arch: &str, accel: Accel) -> Result<Vec<String>, Error> {
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
        Accel::Auto | Accel::Cpu => vec![format!("llama-{RUNTIME_TAG}-bin-ubuntu-{arch}.tar.gz")],
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

pub(super) fn parse_probe(output: &str, expected_marker: &str) -> Probe {
    let mut probe = Probe::default();
    for line in output.lines() {
        let Some(rest) = line.strip_prefix("SOFER ") else {
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
        let Some(rest) = line.strip_prefix("SOFER HASH=") else {
            continue;
        };
        if let Some((name, hash)) = rest.split_once(':') {
            hashes.insert(name.to_string(), hash.to_string());
        }
    }
    hashes
}

pub(super) fn tail_text(output: &str) -> String {
    let lines: Vec<&str> = output
        .lines()
        .filter(|line| !line.starts_with("SOFER"))
        .collect();
    let start = lines.len().saturating_sub(12);
    lines[start..].join("\n")
}
