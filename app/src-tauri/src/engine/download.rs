//! Resumable model downloads: ranged parallel transfers, resume from a partial
//! file, SHA-256 verification and download progress events.

use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use super::manifest::{
    marker_ok, model_files, model_total, model_url, sha256_file, write_marker, ModelFile,
};
use super::proxy::proxy_for_url;
use super::{io_err, DownloadOptions, DownloadProgress, Error, Manager, ModelInfo};

impl Manager {
    pub fn ensure_model(&self, options: &DownloadOptions) -> Result<ModelInfo, Error> {
        let _guard = self.download_lock.lock().unwrap();
        fs::create_dir_all(&self.paths.models).map_err(io_err)?;

        let quality = options.quality;
        let total = model_total(quality);
        if marker_ok(&self.paths.models, quality) && self.model_info(quality).installed {
            return Ok(self.model_info(quality));
        }

        let installed = self.model_info(quality);
        if let Ok(free) = fs2::available_space(&self.paths.models) {
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
            match self.download_parallel(&agent, &url, spec, part, completed_before, total)? {
                ParallelOutcome::Done => return Ok(()),
                ParallelOutcome::RangeUnsupported => {
                    // Probe said 206 but a worker got 200. `download_parallel`
                    // already kept chunk 0 as the contiguous prefix; finish with
                    // the single-stream path.
                }
            }
        }

        self.download_file_single_with(&agent, &url, spec, part, completed_before, total)
    }

    /// Test/helper entry point without a settings override.
    #[cfg(test)]
    pub(super) fn download_file_from(
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

    pub(super) fn download_parallel(
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
            for (index, chunk) in chunks.iter().enumerate() {
                let chunk = *chunk;
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
                        if first_error.is_none() || matches!(first_error, Some(Error::Cancelled)) {
                            first_error = Some(err);
                        }
                    }
                    Err(_) => {
                        abort.store(true, Ordering::Relaxed);
                        if first_error.is_none() {
                            first_error =
                                Some(Error::Download(format!("{} 下载线程异常退出", spec.name)));
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
            Err(err) => return Err(Error::Download(format!("下载 {} 失败：{err}", spec.name))),
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
            let n = reader
                .read(&mut buf)
                .map_err(|e| Error::Download(format!("读取 {} 失败：{e}", spec.name)))?;
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
}

const PARALLEL_MIN_BYTES: u64 = 32 * 1024 * 1024;
pub(super) const PARALLEL_CONNECTIONS: usize = 8;
const CHUNK_RETRIES: u32 = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ParallelOutcome {
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

pub(super) fn chunk_path(part: &Path, index: usize) -> PathBuf {
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

pub(super) fn build_agent(url: &str, proxy_override: Option<&str>) -> Result<ureq::Agent, Error> {
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
    match agent.get(url).set("Range", "bytes=0-0").call() {
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
        Err(err) => Err(Error::Download(format!("检查分片下载支持失败：{err}"))),
    }
}

#[allow(clippy::too_many_arguments)]
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

#[allow(clippy::too_many_arguments)]
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
    Err(last_error.unwrap_or_else(|| Error::Download(format!("下载 {name} 分片失败"))))
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
