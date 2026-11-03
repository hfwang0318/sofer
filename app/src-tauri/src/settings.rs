//! Persisted app settings and the translation to the kernel's `pipeline::Config`.

use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;

use serde::{Deserialize, Deserializer, Serialize};
use tauri::{AppHandle, Manager};

use crate::engine::{
    compose_proxy_url, normalize_proxy_url, split_proxy_url, DownloadOptions, ModelQuality,
    ServerConfig, IMMEDIATE_RELEASE_SECS,
};
use crate::remote::{self, Accel, RemoteConfig, Source};
use sofer::engine::llamacpp;
use sofer::pipeline::{Config, EngineMode};
use sofer::render::Format;

/// Longest custom prompt we persist. The built-in presets are far shorter; this
/// only stops a pasted document from bloating every request.
pub const MAX_PROMPT_CHARS: usize = 2000;

/// Prompt presets offered in Settings → OCR. `Code` is the default and is
/// byte-for-byte identical to the engine's `DEFAULT_PROMPT`, so an untouched
/// install recognizes exactly as before.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum PromptPreset {
    #[default]
    Code,
    Terminal,
    Table,
    Formula,
    Custom,
}

pub const TERMINAL_PROMPT: &str = "OCR: transcribe the terminal output exactly as shown, \
one line for each output line, keeping every symbol and all indentation.";
pub const TABLE_PROMPT: &str = "OCR: transcribe the table row by row, one row per line, \
keeping every cell in its original column order.";
pub const FORMULA_PROMPT: &str = "OCR: transcribe the text exactly as shown and write math \
formulas in LaTeX, keeping their place in the surrounding text.";

impl PromptPreset {
    pub fn from_id(id: &str) -> Option<Self> {
        match id {
            "code" => Some(Self::Code),
            "terminal" => Some(Self::Terminal),
            "table" => Some(Self::Table),
            "formula" => Some(Self::Formula),
            "custom" => Some(Self::Custom),
            _ => None,
        }
    }

    /// The text sent to the engine for this preset. `Custom` falls back to the
    /// default prompt while the custom text is empty, so an incomplete draft
    /// never sends an empty prompt.
    pub fn prompt(self, custom: &str) -> &str {
        match self {
            Self::Code => llamacpp::DEFAULT_PROMPT,
            Self::Terminal => TERMINAL_PROMPT,
            Self::Table => TABLE_PROMPT,
            Self::Formula => FORMULA_PROMPT,
            Self::Custom => {
                let custom = custom.trim();
                if custom.is_empty() {
                    llamacpp::DEFAULT_PROMPT
                } else {
                    custom
                }
            }
        }
    }
}

/// Unknown ids (hand-edited files, future presets) silently fall back to the
/// default instead of rejecting the whole settings file.
fn deserialize_prompt_preset<'de, D>(deserializer: D) -> Result<PromptPreset, D::Error>
where
    D: Deserializer<'de>,
{
    let raw = Option::<String>::deserialize(deserializer)?;
    Ok(raw
        .as_deref()
        .and_then(PromptPreset::from_id)
        .unwrap_or_default())
}

/// Everything the desktop layer owns. Kernel knobs are kept in the same object so
/// there is exactly one settings file and one save path.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Settings {
    // app layer
    pub shortcut: String,
    pub auto_copy: bool,
    /// Show the bottom-centre toast with the copied line count when OCR
    /// finishes while the main window is not in front.
    pub notify_on_result: bool,
    /// macOS only: show an Apple Vision result immediately, then replace it
    /// with the local model's refined text.
    pub fast_preview: bool,
    /// Outline the regions the model recognised on the result thumbnail.
    pub show_boxes: bool,
    pub history_limit: u32,

    // model download
    pub use_mirror: bool,
    pub proxy_url: String,
    pub proxy_username: String,
    pub proxy_password: String,
    pub model_quality: ModelQuality,
    /// Unload the model after this many idle seconds (0 = never,
    /// 1 = right after each capture).
    pub idle_sleep_secs: u64,
    /// Quantize the KV cache to q8_0. On by default on every platform; the
    /// memory saving matters more than the CPU-side speed cost.
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
    /// Prompt preset id; `Custom` reads `custom_prompt`.
    #[serde(deserialize_with = "deserialize_prompt_preset")]
    pub prompt_preset: PromptPreset,
    pub custom_prompt: String,
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
            fast_preview: false,
            show_boxes: false,
            history_limit: 20,
            use_mirror: false,
            proxy_url: String::new(),
            proxy_username: String::new(),
            proxy_password: String::new(),
            model_quality: ModelQuality::Low,
            // Windows keeps the whole model resident in RAM; release it after
            // every capture so idle time does not hold ~1 GB. macOS keeps the
            // 5-minute release because Metal reloads are cheap.
            idle_sleep_secs: if cfg!(target_os = "windows") {
                IMMEDIATE_RELEASE_SECS
            } else {
                300
            },
            kv_cache_quant: true,
            ui_scale: 1.15,
            auto_check_updates: true,
            auto_download_updates: true,
            remote_enabled: false,
            remote_host: String::new(),
            remote_user: String::new(),
            remote_ssh_port: 22,
            remote_key: String::new(),
            remote_dir: "~/.sofer".into(),
            remote_server_port: 8099,
            remote_local_port: 0,
            remote_accel: Accel::Auto,
            remote_gpu_layers: 99,
            remote_source: Source::Auto,
            remote_auto_start: true,
            engine: EngineMode::Auto,
            llama_url: llamacpp::default_url(),
            prompt_preset: PromptPreset::Code,
            custom_prompt: String::new(),
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
        let proxy = compose_proxy_url(&self.proxy_url, &self.proxy_username, &self.proxy_password)?;
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
            image_max_tokens: std::env::var("SOFER_IMAGE_MAX_TOKENS")
                .ok()
                .and_then(|value| value.trim().parse().ok())
                .filter(|value| *value > 0),
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

    /// Effective prompt sent to the engine, derived from the preset.
    pub fn prompt(&self) -> &str {
        self.prompt_preset.prompt(&self.custom_prompt)
    }

    /// Map app settings onto the kernel config. `language` is an empty string in
    /// the UI when the fence language should be guessed.
    pub fn config(&self) -> Config {
        Config {
            engine: self.engine,
            llama_url: Some(self.llama_url.clone()),
            prompt: Some(self.prompt().to_string()),
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
            // Box-drawing reconstruction is on by default; the pass only
            // rewrites a prefix the pixels confirm.
            box_art: true,
            language: {
                let lang = self.language.trim();
                if lang.is_empty() {
                    None
                } else {
                    Some(lang.to_string())
                }
            },
            jobs: self.jobs as usize,
            max_pixels: sofer::preprocess::DEFAULT_MAX_PIXELS,
            trim: true,
            upscale_small: true,
            downscale_large: true,
            vision_token_px: 0.0,
            adaptive_background: true,
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
            eprintln!(
                "settings: {} is invalid ({err}); using defaults",
                path.display()
            );
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
                    object.insert("proxyUsername".into(), serde_json::Value::String(username));
                    object.insert("proxyPassword".into(), serde_json::Value::String(password));
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

    // Older files stored one free-text `prompt`. Keep non-default text as the
    // custom preset; the default text simply maps to the `code` preset.
    if !object.contains_key("promptPreset") {
        let legacy = object
            .get("prompt")
            .and_then(serde_json::Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty() && *value != llamacpp::DEFAULT_PROMPT)
            .map(str::to_string);
        if let Some(prompt) = legacy {
            object.insert(
                "promptPreset".into(),
                serde_json::Value::String("custom".into()),
            );
            if !object.contains_key("customPrompt") {
                object.insert("customPrompt".into(), serde_json::Value::String(prompt));
            }
        }
    }
    object.remove("prompt");
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
        "ALT",
        "OPTION",
        "CONTROL",
        "CTRL",
        "COMMAND",
        "CMD",
        "SUPER",
        "SHIFT",
        "COMMANDORCONTROL",
        "COMMANDORCTRL",
        "CMDORCTRL",
        "CMDORCONTROL",
    ];
    let modifier_count = parts
        .iter()
        .filter(|part| modifiers.contains(&part.to_uppercase().as_str()))
        .count();
    let main_count = parts.len().saturating_sub(modifier_count);
    if parts.is_empty() || parts.len() > 3 || main_count != 1 || modifier_count > 2 {
        return Err("快捷键必须是 1 个主键、1 个修饰键+主键，或 2 个修饰键+主键。".into());
    }
    if settings.max_tokens < 128 || settings.max_tokens > 16384 {
        return Err("最大输出长度应在 128 到 16384 之间。".into());
    }
    if settings.jobs == 0 || settings.jobs > 8 {
        return Err("并行识别数应在 1 到 8 之间。".into());
    }
    // Prompts only reach the model engines; tesseract-mode saves skip these,
    // matching the UI, which hides the prompt fields in that mode.
    if settings.engine != EngineMode::Tesseract {
        let custom_prompt = settings.custom_prompt.trim();
        if custom_prompt.chars().count() > MAX_PROMPT_CHARS {
            return Err(format!("自定义提示词不能超过 {MAX_PROMPT_CHARS} 个字符。"));
        }
        if settings.prompt_preset == PromptPreset::Custom && custom_prompt.is_empty() {
            return Err("自定义提示词不能为空。".into());
        }
    }
    if settings.idle_sleep_secs > 3600 {
        return Err("空闲释放时间应为 0（关闭）或 1–3600 秒。".into());
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
        assert_eq!(
            settings.idle_sleep_secs,
            if cfg!(target_os = "windows") {
                IMMEDIATE_RELEASE_SECS
            } else {
                300
            }
        );
        assert!(settings.kv_cache_quant);
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
        for shortcut in ["Control+Shift", "KeyA+KeyB", "Control+Alt+Shift+KeyQ", ""] {
            settings.shortcut = shortcut.into();
            assert!(validate(&settings).is_err(), "should reject {shortcut}");
        }
    }

    #[test]
    fn download_overrides_are_validated() {
        let mut settings = Settings {
            proxy_url: "127.0.0.1:7890".into(),
            proxy_username: "user".into(),
            proxy_password: "p@ss:word".into(),
            use_mirror: true,
            ..Settings::default()
        };
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
    fn default_prompt_preset_equals_the_engine_prompt() {
        let settings = Settings::default();
        assert_eq!(settings.prompt_preset, PromptPreset::Code);
        assert_eq!(settings.prompt(), llamacpp::DEFAULT_PROMPT);
        assert_eq!(
            settings.config().prompt.as_deref(),
            Some(llamacpp::DEFAULT_PROMPT)
        );
    }

    #[test]
    fn unknown_prompt_preset_falls_back_to_the_default() {
        for raw in [
            r#"{"promptPreset":"from-the-future"}"#,
            r#"{"promptPreset":null}"#,
        ] {
            let settings = parse(raw).unwrap();
            assert_eq!(settings.prompt_preset, PromptPreset::Code, "{raw}");
            assert_eq!(settings.prompt(), llamacpp::DEFAULT_PROMPT);
        }
    }

    #[test]
    fn legacy_free_text_prompt_becomes_the_custom_preset() {
        let settings = parse(
            r#"{
                "shortcut":"CommandOrControl+Shift+2",
                "prompt":"OCR: keep the diff markers and indentation."
            }"#,
        )
        .unwrap();
        assert_eq!(settings.prompt_preset, PromptPreset::Custom);
        assert_eq!(
            settings.custom_prompt,
            "OCR: keep the diff markers and indentation."
        );
        assert_eq!(
            settings.prompt(),
            "OCR: keep the diff markers and indentation."
        );

        // The old default prompt maps back to the `code` preset, not custom.
        let defaulted = parse(&format!(
            r#"{{"prompt":{}}}"#,
            serde_json::to_string(llamacpp::DEFAULT_PROMPT).unwrap()
        ))
        .unwrap();
        assert_eq!(defaulted.prompt_preset, PromptPreset::Code);
        assert_eq!(defaulted.prompt(), llamacpp::DEFAULT_PROMPT);
    }

    #[test]
    fn custom_prompt_length_is_limited() {
        let mut settings = Settings {
            prompt_preset: PromptPreset::Custom,
            custom_prompt: "x".repeat(MAX_PROMPT_CHARS),
            ..Settings::default()
        };
        validate(&settings).unwrap();

        settings.custom_prompt.push('x');
        let error = validate(&settings).unwrap_err();
        assert!(error.contains(&MAX_PROMPT_CHARS.to_string()));

        settings.custom_prompt.clear();
        assert!(
            validate(&settings).is_err(),
            "empty custom prompt is rejected"
        );
        settings.engine = EngineMode::Tesseract;
        validate(&settings).unwrap();
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
        assert!(json.contains("\"promptPreset\":\"code\""));
        assert!(json.contains("\"customPrompt\""));
        assert_eq!(parsed.prompt_preset, settings.prompt_preset);
        assert_eq!(parsed.shortcut, settings.shortcut);
        assert_eq!(parsed.model_quality, settings.model_quality);
        assert_eq!(parsed.engine, settings.engine);
        assert_eq!(parsed.format, settings.format);
    }

    #[test]
    fn validation_rejects_bad_engine_settings() {
        let mut settings = Settings {
            llama_url: "localhost:8099".into(),
            ..Settings::default()
        };
        assert!(validate(&settings).is_err());

        settings.llama_url = "http://127.0.0.1:8099".into();
        settings.max_tokens = 0;
        assert!(validate(&settings).is_err());
    }
}
