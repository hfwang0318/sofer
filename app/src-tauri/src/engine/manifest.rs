//! Pinned model packs: file names, byte sizes, SHA-256 digests, download URLs,
//! and the `verified-*.json` marker that records a finished pack.

use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::{io_err, Error};

pub const MODEL_LICENSE: &str = "MIT";

/// Which weights the app should load. Both packs share the same Q8 mmproj.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum ModelQuality {
    #[default]
    High,
    Low,
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
pub(super) const OFFICIAL_REVISION: &str = "65a42de1148dbed2297e922b5dbc7d9b70c36578";
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

pub(super) const MMPROJ: ModelFile = ModelFile {
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

pub(super) fn model_url_with_base(
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
    let host = if use_mirror {
        MIRROR_HOST
    } else {
        OFFICIAL_HOST
    };
    format!(
        "{host}/{}/resolve/{}/{}",
        file.repo, file.revision, file.name
    )
}
pub(super) fn sha256_file(path: &Path) -> Result<String, Error> {
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

pub(super) fn marker_path(dir: &Path, quality: ModelQuality) -> PathBuf {
    dir.join(format!("verified-{}.json", quality.as_str()))
}

pub(super) fn marker_ok(dir: &Path, quality: ModelQuality) -> bool {
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
                return value.get("revision").and_then(|v| v.as_str()) == Some(OFFICIAL_REVISION);
            }
        }
    }
    false
}

pub(super) fn write_marker(dir: &Path, quality: ModelQuality) -> Result<(), Error> {
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
