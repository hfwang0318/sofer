//! Persisted app settings and the translation to the kernel's `pipeline::Config`.

use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};

use crate::engine::{
    compose_proxy_url, normalize_proxy_url, split_proxy_url, DownloadOptions, ModelQuality,
    ServerConfig,
};
use crate::remote::{self, Accel, RemoteConfig, Source};
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
    /// Show a system notification with a short preview when OCR finishes while
    /// the window is not focused.
    pub notify_on_result: bool,
    pub history_limit: u32,

    // model download
    pub use_mirror: bool,
    pub proxy_url: String,
    pub proxy_username: String,
    pub proxy_password: String,
    pub model_quality: ModelQuality,
    /// Unload the model after this many idle seconds (0 = never).
    pub idle_sleep_secs: u64,
    /// Quantize the KV cache to q8_0. Default on for Apple Metal, off for CPU.
    pub kv_cache_quant: bool,
    pub ui_scale: f64,
    pub auto_check_updates: bool,
    pub auto_download_updates: bool,

    // remote model server (optional; replaces the managed local server)
    pub remote_enabled: bool,
    pub remote_host: String,
    pub remote_user: String,
    pub remote_ssh_port: u16,
    pub remote_key: String,
    pub remote_dir: String,
    pub remote_server_port: u16,
    /// 0 = pick a free local port for the SSH tunnel.
    pub remote_local_port: u16,
    pub remote_accel: Accel,
    pub remote_gpu_layers: u32,
    pub remote_source: Source,
    pub remote_auto_start: bool,

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
            notify_on_result: true,
            history_limit: 20,
            use_mirror: false,
            proxy_url: String::new(),
            proxy_username: String::new(),
            proxy_password: String::new(),
            model_quality: ModelQuality::Low,
            idle_sleep_secs: 300,
            kv_cache_quant: cfg!(target_os = "macos"),
            ui_scale: 1.15,
            auto_check_updates: true,
            auto_download_updates: true,
            remote_enabled: false,
            remote_host: String::new(),
            remote_user: String::new(),
            remote_ssh_port: 22,
            remote_key: String::new(),
            remote_dir: "~/.ocr4code".into(),
            remote_server_port: 8099,
            remote_local_port: 0,
            remote_accel: Accel::Auto,
            remote_gpu_layers: 99,
            remote_source: Source::Auto,
            remote_auto_start: true,
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
    /// Compose the download settings, percent-encoding proxy credentials.
    pub fn download_options(&self) -> Result<DownloadOptions, String> {
        let proxy = compose_proxy_url(
            &self.proxy_url,
            &self.proxy_username,
            &self.proxy_password,
        )?;
        Ok(DownloadOptions {
            quality: self.model_quality,
            use_mirror: self.use_mirror,
            proxy,
        })
    }

    pub fn server_config(&self) -> ServerConfig {
        ServerConfig {
            quality: self.model_quality,
            jobs: self.jobs,
            max_tokens: self.max_tokens,
            kv_cache_quant: self.kv_cache_quant,
            idle_sleep_secs: self.idle_sleep_secs,
        }
    }

    /// Settings that require a llama-server restart when changed. The server
    /// arguments are derived from these, so a save must not leave the old
    /// process running with stale `-c`/`-np`/KV flags.
    pub fn server_signature(&self) -> String {
        format!(
            "{:?}|{:?}|{}|{}|{}|{}",
            self.engine,
            self.model_quality,
            self.jobs,
            self.max_tokens,
            self.kv_cache_quant,
            self.idle_sleep_secs
        )
    }

    /// Build the remote deployment config when the feature is enabled.
    pub fn remote_config(&self) -> Result<Option<RemoteConfig>, String> {
        if !self.remote_enabled {
            return Ok(None);
        }
        let config = RemoteConfig {
            host: self.remote_host.trim().to_string(),
            user: self.remote_user.trim().to_string(),
            ssh_port: self.remote_ssh_port,
            key_path: self.remote_key.trim().to_string(),
            dir: self.remote_dir.trim().to_string(),
            server_port: self.remote_server_port,
            local_port: self.remote_local_port,
            accel: self.remote_accel,
            gpu_layers: self.remote_gpu_layers,
            quality: self.model_quality,
            source: self.remote_source,
            jobs: self.jobs,
            max_tokens: self.max_tokens,
            kv_cache_quant: self.kv_cache_quant,
            idle_sleep_secs: self.idle_sleep_secs,
            use_mirror: self.use_mirror,
        };
        remote::validate(&config).map_err(|err| err.to_string())?;
        Ok(Some(config))
    }

    /// A remote restart is needed when any of these change.
    pub fn remote_signature(&self) -> String {
        format!(
            "{}|{}|{}|{}|{}|{}|{}|{}|{:?}|{}|{:?}|{:?}|{}|{}|{}|{}",
            self.remote_enabled,
            self.remote_host,
            self.remote_user,
            self.remote_ssh_port,
            self.remote_key,
            self.remote_dir,
            self.remote_server_port,
            self.remote_local_port,
            self.remote_accel,
            self.remote_gpu_layers,
            self.model_quality,
            self.remote_source,
            self.jobs,
            self.max_tokens,
            self.kv_cache_quant,
            self.idle_sleep_secs
        )
    }

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
            // The desktop UI has no encoding control; the kernel default (PNG)
            // keeps the bytes sent to the model unchanged.
            image_format: Default::default(),
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
    match parse(&raw) {
        Ok(settings) => settings,
        Err(err) => {
            // Keep the broken file for debugging instead of silently overwriting it.
            eprintln!("settings: {} is invalid ({err}); using defaults", path.display());
            let _ = fs::rename(&path, path.with_extension("json.bak"));
            Settings::default()
        }
    }
}

/// Deserialize settings and migrate the pre-0.2 fields (`downloadProxy`,
/// `modelBase`) into the split proxy + mirror representation.
fn parse(raw: &str) -> Result<Settings, serde_json::Error> {
    let mut value: serde_json::Value = serde_json::from_str(raw)?;
    migrate_legacy(&mut value);
    serde_json::from_value(value)
}

fn migrate_legacy(value: &mut serde_json::Value) {
    let Some(object) = value.as_object_mut() else {
        return;
    };
    if !object.contains_key("proxyUrl") {
        if let Some(legacy) = object
            .get("downloadProxy")
            .and_then(serde_json::Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            match split_proxy_url(legacy) {
                Some((url, username, password)) => {
                    object.insert("proxyUrl".into(), serde_json::Value::String(url));
                    object.insert(
                        "proxyUsername".into(),
                        serde_json::Value::String(username),
                    );
                    object.insert(
                        "proxyPassword".into(),
                        serde_json::Value::String(password),
                    );
                }
                None => {
                    object.insert(
                        "proxyUrl".into(),
                        serde_json::Value::String(legacy.to_string()),
                    );
                }
            }
        }
    }
    if !object.contains_key("useMirror") {
        let mirror = object
            .get("modelBase")
            .and_then(serde_json::Value::as_str)
            .map(str::to_ascii_lowercase)
            .is_some_and(|value| value.contains("hf-mirror.com"));
        if mirror {
            object.insert("useMirror".into(), serde_json::Value::Bool(true));
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
    let shortcut = settings.shortcut.trim();
    if shortcut.is_empty() {
        return Err("快捷键不能为空。".into());
    }
    let parts: Vec<&str> = shortcut
        .split('+')
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .collect();
    let modifiers = [
        "ALT", "OPTION", "CONTROL", "CTRL", "COMMAND", "CMD", "SUPER", "SHIFT",
        "COMMANDORCONTROL", "COMMANDORCTRL", "CMDORCTRL", "CMDORCONTROL",
    ];
    let modifier_count = parts
        .iter()
        .filter(|part| modifiers.contains(&part.to_uppercase().as_str()))
        .count();
    let main_count = parts.len().saturating_sub(modifier_count);
    if parts.is_empty() || parts.len() > 3 || main_count != 1 || modifier_count > 2 {
        return Err(
            "快捷键必须是 1 个主键、1 个修饰键+主键，或 2 个修饰键+主键。".into(),
        );
    }
    if settings.max_tokens < 128 || settings.max_tokens > 16384 {
        return Err("最大输出长度应在 128 到 16384 之间。".into());
    }
    if settings.jobs == 0 || settings.jobs > 8 {
        return Err("并行识别数应在 1 到 8 之间。".into());
    }
    if settings.idle_sleep_secs != 0
        && !(60..=3600).contains(&settings.idle_sleep_secs)
    {
        return Err("空闲释放时间应为 0（关闭）或 60–3600 秒。".into());
    }
    if settings.psm > 13 {
        return Err("tesseract PSM 应在 0 到 13 之间。".into());
    }
    if settings.tess_lang.trim().is_empty() {
        return Err("tesseract 语言不能为空（例如 eng）。".into());
    }
    match compose_proxy_url(
        &settings.proxy_url,
        &settings.proxy_username,
        &settings.proxy_password,
    ) {
        Ok(Some(url)) => {
            ureq::Proxy::new(normalize_proxy_url(&url))
                .map_err(|e| format!("下载代理地址无效：{e}"))?;
        }
        Ok(None) => {}
        Err(err) => return Err(err),
    }

    if !(1.0..=1.3).contains(&settings.ui_scale) {
        return Err("界面缩放应在 100% 到 130% 之间。".into());
    }

    let url = settings.llama_url.trim();
    if !(url.starts_with("http://") || url.starts_with("https://")) {
        return Err("llama.cpp 地址必须以 http:// 或 https:// 开头。".into());
    }
    if settings.remote_enabled {
        let config = settings.remote_config()?.ok_or("远程 OCR 引擎配置无效。")?;
        if config.host.is_empty() {
            return Err("远程服务器地址不能为空。".into());
        }
        if config.user.is_empty() {
            return Err("远程服务器用户名不能为空。".into());
        }
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
        assert_eq!(settings.model_quality, ModelQuality::Low);
        assert_eq!(settings.idle_sleep_secs, 300);
        assert_eq!(settings.kv_cache_quant, cfg!(target_os = "macos"));
        assert!(!settings.use_mirror);
        assert!(settings.proxy_url.is_empty());
        assert!(settings.notify_on_result);
    }

    #[test]
    fn server_signature_covers_context_affecting_knobs() {
        let settings = Settings::default();
        let base = settings.server_signature();
        for mutate in [
            |s: &mut Settings| s.engine = EngineMode::Tesseract,
            |s: &mut Settings| s.model_quality = ModelQuality::High,
            |s: &mut Settings| s.jobs = 1,
            |s: &mut Settings| s.max_tokens = 512,
            |s: &mut Settings| s.kv_cache_quant = !s.kv_cache_quant,
            |s: &mut Settings| s.idle_sleep_secs = 0,
        ] {
            let mut next = settings.clone();
            mutate(&mut next);
            assert_ne!(next.server_signature(), base);
        }
        assert_eq!(settings.server_signature(), base);
    }

    #[test]
    fn shortcut_shape_allows_one_to_three_keys() {
        let mut settings = Settings::default();
        for shortcut in [
            "F8",
            "Control+KeyQ",
            "Control+Shift+Digit2",
            "Command+Alt+KeyQ",
        ] {
            settings.shortcut = shortcut.into();
            assert!(validate(&settings).is_ok(), "should accept {shortcut}");
        }
        for shortcut in [
            "Control+Shift",
            "KeyA+KeyB",
            "Control+Alt+Shift+KeyQ",
            "",
        ] {
            settings.shortcut = shortcut.into();
            assert!(validate(&settings).is_err(), "should reject {shortcut}");
        }
    }

    #[test]
    fn download_overrides_are_validated() {
        let mut settings = Settings::default();
        settings.proxy_url = "127.0.0.1:7890".into();
        settings.proxy_username = "user".into();
        settings.proxy_password = "p@ss:word".into();
        settings.use_mirror = true;
        validate(&settings).unwrap();
        let options = settings.download_options().unwrap();
        assert!(options.use_mirror);
        assert_eq!(options.quality, ModelQuality::Low);
        assert_eq!(
            options.proxy.as_deref(),
            Some("http://user:p%40ss%3Aword@127.0.0.1:7890")
        );
        settings.model_quality = ModelQuality::High;
        assert_eq!(
            settings.download_options().unwrap().quality,
            ModelQuality::High
        );

        settings.proxy_url = "ftp://mirror.example:21".into();
        assert!(validate(&settings).is_err());

        settings.proxy_url.clear();
        assert!(validate(&settings).is_err(), "credentials need an address");

        settings.proxy_username.clear();
        settings.proxy_password.clear();
        validate(&settings).unwrap();
        settings.proxy_url = "http://127.0.0.1:7890".into();
        settings.proxy_password = "secret".into();
        assert!(validate(&settings).is_err(), "password needs a username");
    }

    #[test]
    fn old_settings_without_download_fields_still_load() {
        let settings = parse(r#"{"shortcut":"CommandOrControl+Shift+2"}"#).unwrap();
        assert!(settings.proxy_url.is_empty());
        assert!(!settings.use_mirror);
        assert_eq!(settings.model_quality, ModelQuality::Low);
        assert_eq!(settings.ui_scale, 1.15);
        assert!(settings.notify_on_result);
    }

    #[test]
    fn legacy_proxy_and_mirror_are_migrated() {
        let settings = parse(
            r#"{
                "shortcut":"CommandOrControl+Shift+2",
                "downloadProxy":"http://user:p%40ss@127.0.0.1:7890",
                "modelBase":"https://hf-mirror.com/ggml-org/GLM-OCR-GGUF/resolve/abc"
            }"#,
        )
        .unwrap();
        assert_eq!(settings.proxy_url, "http://127.0.0.1:7890");
        assert_eq!(settings.proxy_username, "user");
        assert_eq!(settings.proxy_password, "p@ss");
        assert!(settings.use_mirror);
    }

    #[test]
    fn settings_round_trip_as_camel_case() {
        let settings = Settings::default();
        let json = serde_json::to_string(&settings).unwrap();
        let parsed: Settings = serde_json::from_str(&json).unwrap();

        assert!(json.contains("\"maxTokens\""));
        assert!(json.contains("\"autoCopy\""));
        assert!(json.contains("\"notifyOnResult\""));
        assert!(json.contains("\"proxyUrl\""));
        assert!(json.contains("\"proxyUsername\""));
        assert!(json.contains("\"proxyPassword\""));
        assert!(json.contains("\"useMirror\""));
        assert!(json.contains("\"modelQuality\""));
        assert!(json.contains("\"idleSleepSecs\""));
        assert!(json.contains("\"kvCacheQuant\""));
        assert_eq!(parsed.shortcut, settings.shortcut);
        assert_eq!(parsed.model_quality, settings.model_quality);
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
