//! Persisted app settings and the translation to the kernel's `pipeline::Config`.

use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};

use crate::engine::normalize_proxy_url;
use ocr4code::engine::llamacpp;
use ocr4code::pipeline::{Config, EngineMode};
use ocr4code::render::Format;

/// Everything the desktop layer owns. Kernel knobs are kept in the same object so
/// there is exactly one settings file and one save path.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Settings {
    // app layer
    pub shortcut: String,
    pub auto_copy: bool,
    pub history_limit: u32,

    // model download
    pub download_proxy: String,
    pub model_base: String,
    pub ui_scale: f64,

    // kernel: engine
    pub engine: EngineMode,
    pub llama_url: String,
    pub prompt: String,
    pub max_tokens: u32,
    pub tess_lang: String,
    pub psm: u32,

    // kernel: layout
    pub tile: bool,
    pub geometry: bool,
    pub join_wrap: bool,
    pub keep_gutter: bool,

    // kernel: output
    pub format: Format,
    pub language: String,
    pub jobs: u32,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            shortcut: "CommandOrControl+Shift+2".into(),
            auto_copy: true,
            history_limit: 20,
            download_proxy: String::new(),
            model_base: String::new(),
            ui_scale: 1.15,
            engine: EngineMode::Auto,
            llama_url: llamacpp::default_url(),
            prompt: llamacpp::DEFAULT_PROMPT.into(),
            max_tokens: 3072,
            tess_lang: "eng".into(),
            psm: 6,
            tile: true,
            geometry: true,
            join_wrap: false,
            keep_gutter: false,
            format: Format::Text,
            language: String::new(),
            jobs: 3,
        }
    }
}

impl Settings {
    /// Map app settings onto the kernel config. `language` is an empty string in
    /// the UI when the fence language should be guessed.
    pub fn config(&self) -> Config {
        Config {
            engine: self.engine,
            llama_url: Some(self.llama_url.clone()),
            prompt: Some(self.prompt.clone()),
            max_tokens: self.max_tokens,
            tile: self.tile,
            tess_lang: self.tess_lang.clone(),
            psm: self.psm,
            no_geometry: !self.geometry,
            join_wrap: self.join_wrap,
            keep_gutter: self.keep_gutter,
            format: self.format,
            language: {
                let lang = self.language.trim();
                if lang.is_empty() {
                    None
                } else {
                    Some(lang.to_string())
                }
            },
            jobs: self.jobs as usize,
            mock_text: None,
            mock_text_file: None,
        }
    }
}

/// Runtime state owned by Tauri. Capture runs off the UI thread; `busy` keeps two
/// global-shortcut presses from stacking selections.
pub struct AppState {
    pub settings: Mutex<Settings>,
    pub busy: std::sync::atomic::AtomicBool,
    /// Set when the shortcut is pressed while a selection is open.
    pub cancel: std::sync::atomic::AtomicBool,
    /// Whether the global shortcut was actually claimed at startup / on save.
    pub shortcut_active: std::sync::atomic::AtomicBool,
}

pub fn path(app: &AppHandle) -> PathBuf {
    app.path()
        .app_config_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .join("settings.json")
}

pub fn load(app: &AppHandle) -> Settings {
    let path = path(app);
    let Ok(raw) = fs::read_to_string(&path) else {
        return Settings::default();
    };
    match serde_json::from_str::<Settings>(&raw) {
        Ok(settings) => settings,
        Err(err) => {
            // Keep the broken file for debugging instead of silently overwriting it.
            eprintln!("settings: {} is invalid ({err}); using defaults", path.display());
            let _ = fs::rename(&path, path.with_extension("json.bak"));
            Settings::default()
        }
    }
}

pub fn save(app: &AppHandle, settings: &Settings) -> Result<(), String> {
    let path = path(app);
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).map_err(|e| format!("creating {}: {e}", dir.display()))?;
    }
    let body = serde_json::to_string_pretty(settings).map_err(|e| e.to_string())?;
    fs::write(&path, body).map_err(|e| format!("writing {}: {e}", path.display()))
}

/// Cheap guard rails; the UI validates the same fields for inline feedback.
pub fn validate(settings: &Settings) -> Result<(), String> {
    if settings.shortcut.trim().is_empty() {
        return Err("快捷键不能为空。".into());
    }
    if settings.max_tokens < 128 || settings.max_tokens > 16384 {
        return Err("最大 token 数应在 128 到 16384 之间。".into());
    }
    if settings.jobs == 0 || settings.jobs > 8 {
        return Err("并发数应在 1 到 8 之间。".into());
    }
    if settings.psm > 13 {
        return Err("Tesseract PSM 应在 0 到 13 之间。".into());
    }
    if settings.tess_lang.trim().is_empty() {
        return Err("Tesseract 语言不能为空（例如 eng）。".into());
    }
    let proxy = settings.download_proxy.trim();
    if !proxy.is_empty() {
        if !proxy.starts_with("http://") {
            return Err("下载代理必须以 http:// 开头。".into());
        }
        ureq::Proxy::new(normalize_proxy_url(proxy))
            .map_err(|e| format!("下载代理地址无效：{e}"))?;
    }
    let model_base = settings.model_base.trim();
    if !model_base.is_empty()
        && !model_base.starts_with("http://")
        && !model_base.starts_with("https://")
    {
        return Err("模型镜像必须以 http:// 或 https:// 开头。".into());
    }

    if !(1.0..=1.3).contains(&settings.ui_scale) {
        return Err("界面缩放应在 100% 到 130% 之间。".into());
    }

    let url = settings.llama_url.trim();
    if !(url.starts_with("http://") || url.starts_with("https://")) {
        return Err("llama.cpp 地址必须以 http:// 或 https:// 开头。".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_map_to_the_kernel_config() {
        let settings = Settings::default();
        let config = settings.config();

        assert_eq!(config.engine, EngineMode::Auto);
        assert_eq!(config.max_tokens, 3072);
        assert_eq!(config.jobs, 3);
        assert_eq!(config.language, None);
        assert!(!config.no_geometry);
        assert!(!config.join_wrap);
        assert!(config.tile);
    }

    #[test]
    fn download_overrides_are_validated() {
        let mut settings = Settings::default();
        settings.download_proxy = "http://user:p%40ss@127.0.0.1:7890".into();
        settings.model_base =
            "https://hf-mirror.com/ggml-org/GLM-OCR-GGUF/resolve/abc".into();
        validate(&settings).unwrap();
        settings.download_proxy = "127.0.0.1:7890".into();
        assert!(validate(&settings).is_err());
        settings.download_proxy.clear();
        settings.model_base = "ftp://mirror.example/models".into();
        assert!(validate(&settings).is_err());
    }

    #[test]
    fn old_settings_without_download_fields_still_load() {
        let settings: Settings = serde_json::from_str(r#"{"shortcut":"CommandOrControl+Shift+2"}"#).unwrap();
        assert_eq!(settings.download_proxy, "");
        assert_eq!(settings.model_base, "");
        assert_eq!(settings.ui_scale, 1.15);
    }

    #[test]
    fn settings_round_trip_as_camel_case() {
        let settings = Settings::default();
        let json = serde_json::to_string(&settings).unwrap();
        let parsed: Settings = serde_json::from_str(&json).unwrap();

        assert!(json.contains("\"maxTokens\""));
        assert!(json.contains("\"autoCopy\""));
        assert!(json.contains("\"downloadProxy\""));
        assert!(json.contains("\"modelBase\""));
        assert_eq!(parsed.shortcut, settings.shortcut);
        assert_eq!(parsed.engine, settings.engine);
        assert_eq!(parsed.format, settings.format);
    }

    #[test]
    fn validation_rejects_bad_engine_settings() {
        let mut settings = Settings::default();
        settings.llama_url = "localhost:8099".into();
        assert!(validate(&settings).is_err());

        settings.llama_url = "http://127.0.0.1:8099".into();
        settings.max_tokens = 0;
        assert!(validate(&settings).is_err());
    }
}
