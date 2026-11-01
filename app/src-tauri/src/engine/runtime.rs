//! Locate the bundled (or system) `llama-server` runtime and report its version.

use std::fs;
use std::path::{Path, PathBuf};

use super::{Manager, RuntimeInfo};

pub const RUNTIME_VERSION: &str = "llama.cpp b12018";

impl Manager {
    pub(super) fn runtime_executable(&self) -> Option<PathBuf> {
        if self.bundled_runtime_ready() {
            return Some(bundled_executable(&self.paths.runtime));
        }
        self.system_runtime.clone()
    }

    pub(super) fn bundled_runtime_ready(&self) -> bool {
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

pub(super) fn bundled_executable(runtime_dir: &Path) -> PathBuf {
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

pub(super) fn find_in_path(name: &str) -> Option<PathBuf> {
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
