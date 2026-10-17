use serde::{Deserialize, Serialize};
use std::io::Write;
use std::path::{Path, PathBuf};
use ts_rs::TS;

/// Format version of `config.json`.
///
/// 1. snake_case keys.
/// 2. camelCase keys, matching the IPC wire format.
pub const CURRENT_CONFIG_VERSION: u32 = 2;

fn default_config_version() -> u32 {
    1
}

/// `extra_params` is merged into the request body, so it has to be an object.
///
/// `#[serde(default)]` on a `serde_json::Value` yields `Null`, not `{}`. A
/// provider created through `update_provider` therefore ended up with `null`,
/// `merge_extra_params` skipped it as a non-object, and the seeded defaults —
/// including the `chat_template_kwargs.enable_thinking = false` that vLLM and
/// Qwen need — were silently dropped.
pub fn default_extra_params() -> serde_json::Value {
    serde_json::json!({})
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "types.ts")]
pub struct ProviderConfig {
    pub name: String,
    pub base_url: String,
    pub api_key: String,
    pub model: String,
    #[serde(default = "default_system_prompt")]
    pub system_prompt: String,
    #[serde(default = "default_user_prompt")]
    pub user_prompt: String,
    #[serde(default = "default_extra_params")]
    #[ts(type = "Record<string, unknown>")]
    pub extra_params: serde_json::Value,
}

pub fn default_system_prompt() -> String {
    "You are a translator. Translate the following text from {from} to {to}. Output ONLY the translation, nothing else.".to_string()
}

pub fn default_user_prompt() -> String {
    "{text}".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "types.ts")]
pub struct AppConfig {
    /// Set when the file is loaded; see [`CURRENT_CONFIG_VERSION`].
    #[serde(default = "default_config_version")]
    pub version: u32,
    pub enabled: bool,
    #[serde(default)]
    pub debug_logging: bool,
    #[serde(default = "default_selection_trigger_enabled")]
    pub selection_trigger_enabled: bool,
    pub source_lang: String,
    pub target_lang: String,
    pub active_provider: String,
    #[serde(default = "default_mouse_trigger_button")]
    #[ts(type = "number")]
    pub mouse_trigger_button: i64,
    pub providers: Vec<ProviderConfig>,
}

pub fn default_mouse_trigger_button() -> i64 {
    2
}

/// A partial update to [`AppConfig`].
///
/// This replaced a `serde_json::Value` whose keys were looked up by hand with
/// `updates.get("source_lang")`. A misspelled or wrongly-cased key was a silent
/// no-op that still reported success, so the settings UI could show a change the
/// app had not made. A typed patch makes that a deserialization error, and
/// `deny_unknown_fields` turns a stray key into an error rather than a shrug.
#[derive(Debug, Clone, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[ts(export, export_to = "types.ts")]
pub struct ConfigPatch {
    #[ts(optional)]
    pub enabled: Option<bool>,
    #[ts(optional)]
    pub debug_logging: Option<bool>,
    #[ts(optional)]
    pub selection_trigger_enabled: Option<bool>,
    #[ts(optional)]
    pub source_lang: Option<String>,
    #[ts(optional)]
    pub target_lang: Option<String>,
    #[ts(optional)]
    pub active_provider: Option<String>,
    #[ts(optional)]
    #[ts(type = "number")]
    pub mouse_trigger_button: Option<i64>,
}

/// The lowest mouse button number that may be bound to translation.
///
/// Button 0 is the left button. Binding it would make every click in every app
/// also a translation request, so it is refused rather than accepted and then
/// never matched.
pub const MIN_TRIGGER_BUTTON: i64 = 1;
pub const MAX_TRIGGER_BUTTON: i64 = 31;

impl ConfigPatch {
    /// Apply to `config`, rejecting values that would leave the app in a state
    /// where it cannot work — a bound mouse button that never matches, or a
    /// provider that does not exist.
    pub fn apply_to(&self, config: &mut AppConfig) -> Result<(), String> {
        if let Some(enabled) = self.enabled {
            config.enabled = enabled;
        }
        if let Some(debug_logging) = self.debug_logging {
            config.debug_logging = debug_logging;
        }
        if let Some(selection_trigger_enabled) = self.selection_trigger_enabled {
            config.selection_trigger_enabled = selection_trigger_enabled;
        }
        if let Some(lang) = &self.source_lang {
            config.source_lang = require_language_code(lang, "源语言")?;
        }
        if let Some(lang) = &self.target_lang {
            config.target_lang = require_language_code(lang, "目标语言")?;
        }
        if let Some(provider) = &self.active_provider {
            if !config
                .providers
                .iter()
                .any(|candidate| &candidate.name == provider)
            {
                return Err(format!("未知的翻译服务：{provider}"));
            }
            config.active_provider = provider.clone();
        }
        if let Some(button) = self.mouse_trigger_button {
            if !(MIN_TRIGGER_BUTTON..=MAX_TRIGGER_BUTTON).contains(&button) {
                return Err(format!(
                    "触发键必须在 {MIN_TRIGGER_BUTTON} 到 {MAX_TRIGGER_BUTTON} 之间\
                     （0 是左键），现在是 {button}"
                ));
            }
            config.mouse_trigger_button = button;
        }
        Ok(())
    }
}

fn require_language_code(value: &str, field: &str) -> Result<String, String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(format!("{field}不能为空"));
    }
    Ok(trimmed.to_string())
}

pub fn default_selection_trigger_enabled() -> bool {
    true
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            version: CURRENT_CONFIG_VERSION,
            enabled: true,
            debug_logging: false,
            selection_trigger_enabled: default_selection_trigger_enabled(),
            source_lang: "auto".to_string(),
            target_lang: "zh".to_string(),
            active_provider: "openai".to_string(),
            mouse_trigger_button: default_mouse_trigger_button(),
            providers: vec![
                ProviderConfig {
                    name: "deeplx".into(),
                    base_url: "http://127.0.0.1:1188".into(),
                    api_key: String::new(),
                    model: String::new(),
                    system_prompt: String::new(),
                    user_prompt: String::new(),
                    extra_params: serde_json::json!({}),
                },
                ProviderConfig {
                    name: "openai".into(),
                    base_url: "https://api.openai.com/v1".into(),
                    api_key: String::new(),
                    model: "gpt-4o-mini".into(),
                    system_prompt: default_system_prompt(),
                    user_prompt: default_user_prompt(),
                    extra_params: serde_json::json!({
                        "chat_template_kwargs": { "enable_thinking": false }
                    }),
                },
                ProviderConfig {
                    name: "gemini".into(),
                    base_url: "https://generativelanguage.googleapis.com/v1beta".into(),
                    api_key: String::new(),
                    model: "gemini-2.0-flash".into(),
                    system_prompt: default_system_prompt(),
                    user_prompt: default_user_prompt(),
                    extra_params: serde_json::json!({}),
                },
                ProviderConfig {
                    name: "claude".into(),
                    base_url: "https://api.anthropic.com".into(),
                    api_key: String::new(),
                    model: "claude-haiku-4-5-20251001".into(),
                    system_prompt: default_system_prompt(),
                    user_prompt: default_user_prompt(),
                    extra_params: serde_json::json!({}),
                },
                ProviderConfig {
                    name: "ollama".into(),
                    // Ollama's OpenAI-compatible endpoint, so it needs no key and
                    // uses the same request path as every other provider.
                    base_url: "http://127.0.0.1:11434/v1".into(),
                    api_key: String::new(),
                    // A translation-specialised model rather than a general one.
                    model: "translategemma:4b".into(),
                    system_prompt: default_system_prompt(),
                    user_prompt: default_user_prompt(),
                    // Turns reasoning off. Harmless for a plain translation model
                    // and necessary for a thinking one: otherwise the popup would
                    // show the model's reasoning alongside the translation.
                    extra_params: serde_json::json!({ "reasoning_effort": "none" }),
                },
            ],
        }
    }
}

fn is_blank(value: &str) -> bool {
    value.trim().is_empty()
}

/// Whether an endpoint lives on this machine.
///
/// Local endpoints take no API key: Ollama, LM Studio, and a self-hosted vLLM
/// have no use for an `Authorization` header. Readiness must not demand one, and
/// rather than special-case provider names the URL decides.
pub fn is_loopback_url(url: &str) -> bool {
    let without_scheme = url.split_once("://").map_or(url, |(_, rest)| rest);
    // Everything up to the path, i.e. the authority (optionally `user:pass@`).
    let authority = without_scheme
        .split(['/', '?', '#'])
        .next()
        .unwrap_or("")
        .to_ascii_lowercase();
    let authority = authority
        .rsplit_once('@')
        .map_or(authority.as_str(), |(_, host)| host);

    // An IPv6 literal is bracketed, and splitting on `:` without honouring the
    // brackets yields an empty string for `[::1]:8080`.
    let host = match authority.strip_prefix('[') {
        Some(rest) => rest.split(']').next().unwrap_or(""),
        None => authority.split(':').next().unwrap_or(""),
    };

    matches!(host, "localhost" | "127.0.0.1" | "::1" | "0.0.0.0") || host.ends_with(".localhost")
}

pub fn validate_provider(provider: &ProviderConfig) -> Result<(), String> {
    let name = provider.name.as_str();

    if !crate::translate::SUPPORTED_PROVIDERS.contains(&name) {
        return Err(format!(
            "未知的翻译服务：{name}（支持：{}）",
            crate::translate::SUPPORTED_PROVIDERS.join("、")
        ));
    }

    if is_blank(&provider.base_url) {
        return Err("还没填 Base URL".to_string());
    }

    // DeepLX translates on its own; every other provider needs a model.
    if name != "deeplx" && is_blank(&provider.model) {
        return Err("还没填模型".to_string());
    }

    // A key is only required where one could actually be used. Pointing at a
    // remote endpoint without one is worth flagging before the user tries it.
    if !is_loopback_url(&provider.base_url) && is_blank(&provider.api_key) {
        return Err("还没填 API Key".to_string());
    }

    Ok(())
}

pub fn validate_active_provider(config: &AppConfig) -> Result<(), String> {
    let provider = config
        .providers
        .iter()
        .find(|provider| provider.name == config.active_provider)
        .ok_or_else(|| format!("Provider '{}' not found", config.active_provider))?;

    validate_provider(provider)
}

/// Why the config file could not be used. Kept after loading so diagnostics can
/// say what happened, instead of the app silently showing defaults and looking
/// like the user's settings reset themselves.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "types.ts")]
pub struct ConfigLoadProblem {
    pub path: String,
    pub kind: ConfigLoadProblemKind,
    pub detail: String,
    /// Where the unreadable file was preserved, when it could be moved aside.
    pub backup_path: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "types.ts")]
pub enum ConfigLoadProblemKind {
    /// The config directory or file could not be read at all.
    Unreadable,
    /// The file was read but is not valid config, e.g. truncated by a crash.
    Malformed,
}

/// The result of reading the config file: a usable config plus, when something
/// went wrong, the reason.
pub struct ConfigLoad {
    pub config: AppConfig,
    pub problem: Option<ConfigLoadProblem>,
    /// Changes made while loading, such as a provider entry that no longer
    /// exists. Surfaced in Diagnostics because a migration the user cannot see
    /// is indistinguishable from the app quietly editing their settings.
    pub notices: Vec<String>,
}

fn config_dir() -> Result<PathBuf, String> {
    dirs::config_dir()
        .map(|dir| dir.join("fk-trans"))
        .ok_or_else(|| "找不到用户配置目录".to_string())
}

pub fn config_path() -> Result<PathBuf, String> {
    Ok(config_dir()?.join("config.json"))
}

pub fn load_config() -> ConfigLoad {
    match config_path() {
        Ok(path) => load_config_from(&path),
        Err(detail) => ConfigLoad {
            config: AppConfig::default(),
            problem: Some(ConfigLoadProblem {
                path: "<unresolved>".to_string(),
                kind: ConfigLoadProblemKind::Unreadable,
                detail,
                backup_path: None,
            }),
            notices: Vec::new(),
        },
    }
}

pub fn load_config_from(path: &Path) -> ConfigLoad {
    let json = match std::fs::read_to_string(path) {
        Ok(json) => json,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            // A first run is not a problem worth reporting.
            return ConfigLoad {
                config: AppConfig::default(),
                problem: None,
                notices: Vec::new(),
            };
        }
        Err(error) => {
            let detail = error.to_string();
            log::error!("[config] Could not read {}: {}", path.display(), detail);
            return ConfigLoad {
                config: AppConfig::default(),
                problem: Some(ConfigLoadProblem {
                    path: path.display().to_string(),
                    kind: ConfigLoadProblemKind::Unreadable,
                    detail,
                    backup_path: None,
                }),
                notices: Vec::new(),
            };
        }
    };

    match crate::compat::from_str_compat::<AppConfig>(&json) {
        Ok(mut config) => {
            tighten_config_permissions(path);
            if config.version < CURRENT_CONFIG_VERSION {
                log::info!(
                    "[config] Migrating {} from format v{} to v{}; it will be written back in the new format on the next save",
                    path.display(),
                    config.version,
                    CURRENT_CONFIG_VERSION
                );
            }
            // In memory the config is always the current format, so any save
            // writes the current format without every caller having to know.
            config.version = CURRENT_CONFIG_VERSION;
            normalize_extra_params(&mut config, path);
            let notices = migrate_providers(&mut config, path);
            ConfigLoad {
                config,
                problem: None,
                notices,
            }
        }
        Err(error) => {
            let detail = error.to_string();
            log::error!(
                "[config] {} is not valid config, keeping it and starting from defaults: {}",
                path.display(),
                detail
            );
            // Do not overwrite what might still be recoverable by hand: move it
            // somewhere obvious and report where.
            let backup = backup_unreadable_config(path);
            ConfigLoad {
                config: AppConfig::default(),
                problem: Some(ConfigLoadProblem {
                    path: path.display().to_string(),
                    kind: ConfigLoadProblemKind::Malformed,
                    detail,
                    backup_path: backup,
                }),
                notices: Vec::new(),
            }
        }
    }
}

/// Bring provider entries written by earlier builds up to date.
///
/// Returns a human-readable notice per change. These are surfaced rather than
/// only logged: silently dropping an entry the user configured, or silently
/// repointing one, is the same class of surprise as a setting that resets
/// itself.
fn migrate_providers(config: &mut AppConfig, path: &Path) -> Vec<String> {
    let mut notices = Vec::new();

    // `custom_http` no longer exists. Its URL-template mode had no stable
    // schema, guessed at the response shape, and could send an API key to any
    // address. Anything it could do, an OpenAI-compatible entry with a different
    // base_url does with a defined request and response.
    let removed: Vec<String> = config
        .providers
        .iter()
        .filter(|provider| !crate::translate::SUPPORTED_PROVIDERS.contains(&provider.name.as_str()))
        .map(|provider| provider.name.clone())
        .collect();
    if !removed.is_empty() {
        config
            .providers
            .retain(|provider| crate::translate::SUPPORTED_PROVIDERS.contains(&provider.name.as_str()));
        for name in removed {
            let notice = format!(
                "Removed the '{name}' provider entry: it is no longer supported. \
                 An OpenAI-compatible provider pointed at the same endpoint covers the same case."
            );
            log::warn!("[config] {}: {}", path.display(), notice);
            notices.push(notice);
        }
    }

    // Ollama is reached over its OpenAI-compatible endpoint now, so the base URL
    // needs the `/v1` path. Without this the old native URL would be asked for
    // `/chat/completions` and return 404.
    for provider in &mut config.providers {
        if provider.name != "ollama" {
            continue;
        }
        let base = provider.base_url.trim().trim_end_matches('/');
        if base.is_empty() || base.contains("/v1") {
            continue;
        }
        let updated = format!("{base}/v1");
        let notice = format!(
            "已把 Ollama 的地址从 `{base}` 改成 `{updated}`：Ollama 走 OpenAI 兼容接口。"
        );
        log::info!("[config] {}: {}", path.display(), notice);
        provider.base_url = updated;
        notices.push(notice);
    }

    notices
}

/// Move an unparseable config aside so a later save cannot overwrite it. Returns
/// the backup path when the move succeeded.
fn backup_unreadable_config(path: &Path) -> Option<String> {
    let file_name = path.file_name()?.to_string_lossy().into_owned();
    let backup = path.with_file_name(format!(
        "{}.unreadable-{}",
        file_name,
        chrono::Local::now().format("%Y%m%d-%H%M%S")
    ));
    match std::fs::rename(path, &backup) {
        Ok(()) => {
            log::warn!("[config] Kept the unreadable config at {}", backup.display());
            Some(backup.display().to_string())
        }
        Err(error) => {
            log::error!("[config] Could not preserve the unreadable config: {}", error);
            None
        }
    }
}

/// An explicit `null` survives `serde(default)`, so configs written by earlier
/// builds can still carry one. Treat it as "no extra params" rather than leaving
/// a value that the merge step skips.
fn normalize_extra_params(config: &mut AppConfig, path: &Path) {
    for provider in &mut config.providers {
        if provider.extra_params.is_null() {
            log::info!(
                "[config] {}: provider '{}' had null extra_params, treating it as empty",
                path.display(),
                provider.name
            );
            provider.extra_params = default_extra_params();
        }
    }
}

pub fn save_config(config: &AppConfig) -> Result<(), String> {
    let path = config_path()?;
    save_config_to(&path, config)
}

pub fn save_config_to(path: &Path, config: &AppConfig) -> Result<(), String> {
    let json = serde_json::to_string_pretty(config)
        .map_err(|e| format!("Failed to serialize config: {}", e))?;
    write_private_atomically(path, json.as_bytes())
}

/// Write `contents` so a reader sees either the old file or the new one, never a
/// half-written one, with the file readable only by its owner.
///
/// `config.json` holds API keys in plain text and used to be written with a bare
/// `fs::write`. A crash, a full disk, or a power cut part-way through truncated
/// it, and the next startup then silently replaced the user's whole
/// configuration with defaults.
fn write_private_atomically(path: &Path, contents: &[u8]) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| format!("Config path {} has no parent directory", path.display()))?;
    std::fs::create_dir_all(parent)
        .map_err(|e| format!("Failed to create {}: {}", parent.display(), e))?;

    // A sibling path, so the rename below stays within one filesystem and is
    // therefore atomic.
    let temp = parent.join(format!(
        ".{}.tmp-{}",
        path.file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| "config".to_string()),
        std::process::id()
    ));

    {
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create(true).truncate(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            // Applied at creation, so the keys are never briefly world-readable.
            options.mode(0o600);
        }
        let mut file = options
            .open(&temp)
            .map_err(|e| format!("Failed to open {}: {}", temp.display(), e))?;
        file.write_all(contents)
            .map_err(|e| format!("Failed to write {}: {}", temp.display(), e))?;
        // Flush to disk before the rename, so the rename cannot publish a file
        // whose contents have not landed yet.
        file.sync_all()
            .map_err(|e| format!("Failed to flush {}: {}", temp.display(), e))?;
    }

    std::fs::rename(&temp, path).map_err(|e| {
        let _ = std::fs::remove_file(&temp);
        format!(
            "Failed to move {} into place at {}: {}",
            temp.display(),
            path.display(),
            e
        )
    })
}

/// Narrow an existing config file to owner-only. Configs written before this
/// existed are group- and world-readable, and they hold API keys.
fn tighten_config_permissions(path: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        let Ok(metadata) = std::fs::metadata(path) else {
            return;
        };
        let mode = metadata.permissions().mode() & 0o777;
        if mode & 0o077 == 0 {
            return;
        }
        log::warn!(
            "[config] {} was readable by other users (mode {:o}), restricting to 0600",
            path.display(),
            mode
        );
        if let Err(error) = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)) {
            log::warn!("[config] Could not restrict config permissions: {}", error);
        }
    }
    #[cfg(not(unix))]
    let _ = path;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn provider(name: &str, base_url: &str, api_key: &str, model: &str) -> ProviderConfig {
        ProviderConfig {
            name: name.to_string(),
            base_url: base_url.to_string(),
            api_key: api_key.to_string(),
            model: model.to_string(),
            system_prompt: default_system_prompt(),
            user_prompt: default_user_prompt(),
            extra_params: serde_json::json!({}),
        }
    }

    #[test]
    fn mouse_trigger_button_defaults_to_middle_button() {
        let config = AppConfig::default();

        assert_eq!(config.mouse_trigger_button, 2);
    }

    #[test]
    fn debug_logging_defaults_to_off() {
        let config = AppConfig::default();

        assert!(!config.debug_logging);
        assert!(config.selection_trigger_enabled);
    }

    /// A file from an older build: snake_case keys and no `mouseTriggerButton`.
    /// It has to go through the compat reader, which is the only place allowed
    /// to accept the old spellings.
    #[test]
    fn a_legacy_config_without_mouse_trigger_button_uses_the_default() {
        let json = r#"{
            "enabled": true,
            "source_lang": "auto",
            "target_lang": "zh",
            "active_provider": "openai",
            "providers": []
        }"#;

        let config: AppConfig = crate::compat::from_str_compat(json).unwrap();

        assert_eq!(config.mouse_trigger_button, 2);
        assert!(!config.debug_logging);
        assert!(config.selection_trigger_enabled);
        assert_eq!(config.source_lang, "auto");
    }

    /// The struct itself must not accept the old spellings: on the IPC path a
    /// snake_case key has to be a hard error, not something silently honoured.
    #[test]
    fn the_current_struct_rejects_legacy_keys() {
        let json = r#"{
            "enabled": true,
            "source_lang": "auto",
            "target_lang": "zh",
            "active_provider": "openai",
            "providers": []
        }"#;

        assert!(
            serde_json::from_str::<AppConfig>(json).is_err(),
            "only compat::from_str_compat may accept snake_case keys"
        );
    }

    #[test]
    fn debug_logging_and_mouse_trigger_button_round_trip_through_json() {
        let config = AppConfig {
            debug_logging: true,
            selection_trigger_enabled: false,
            mouse_trigger_button: 4,
            ..AppConfig::default()
        };

        let json = serde_json::to_string(&config).unwrap();
        // The wire and on-disk format is camelCase, and only camelCase.
        assert!(json.contains("\"mouseTriggerButton\":4"), "got {json}");
        assert!(json.contains("\"debugLogging\":true"), "got {json}");
        assert!(!json.contains("mouse_trigger_button"));
        assert_eq!(config.version, CURRENT_CONFIG_VERSION);

        let loaded: AppConfig = serde_json::from_str(&json).unwrap();

        assert_eq!(loaded.mouse_trigger_button, 4);
        assert!(loaded.debug_logging);
        assert!(!loaded.selection_trigger_enabled);
    }

    #[test]
    fn default_config_is_not_ready_without_openai_key() {
        let config = AppConfig::default();

        assert!(validate_active_provider(&config).is_err());
    }

    #[test]
    fn missing_active_provider_is_not_ready() {
        let config = AppConfig {
            active_provider: "missing".to_string(),
            ..AppConfig::default()
        };

        assert!(validate_active_provider(&config).is_err());
    }

    #[test]
    fn openai_compatible_config_is_ready_with_key_model_and_base_url() {
        let config = AppConfig {
            active_provider: "openai".to_string(),
            providers: vec![provider(
                "openai",
                "https://api.example.com/v1",
                "sk-test",
                "model",
            )],
            ..AppConfig::default()
        };

        assert!(validate_active_provider(&config).is_ok());
    }

    /// A local endpoint has no use for an API key, so readiness must not demand
    /// one — demanding it is what stopped Ollama from working at all.
    #[test]
    fn a_local_provider_is_ready_without_an_api_key() {
        let ollama = AppConfig {
            active_provider: "ollama".to_string(),
            providers: vec![provider(
                "ollama",
                "http://127.0.0.1:11434/v1",
                "",
                "translategemma:4b",
            )],
            ..AppConfig::default()
        };
        let deeplx = AppConfig {
            active_provider: "deeplx".to_string(),
            providers: vec![provider("deeplx", "http://127.0.0.1:1188", "", "")],
            ..AppConfig::default()
        };

        assert!(validate_active_provider(&deeplx).is_ok());
        assert!(
            validate_active_provider(&ollama).is_ok(),
            "got: {:?}",
            validate_active_provider(&ollama)
        );
    }

    /// Loopback is decided by the URL, not by the provider's name, so a local
    /// endpoint under any supported provider is treated the same way.
    #[test]
    fn loopback_endpoints_are_recognised_in_their_usual_spellings() {
        for url in [
            "http://127.0.0.1:11434/v1",
            "http://localhost:11434/v1",
            "https://LocalHost/v1",
            "http://[::1]:8080/v1",
            "http://0.0.0.0:8000",
            "http://user:pass@127.0.0.1:8080/v1",
            "127.0.0.1:8000/v1",
            "http://box.localhost/v1",
        ] {
            assert!(is_loopback_url(url), "should be loopback: {url}");
        }

        for url in [
            "https://api.openai.com/v1",
            "https://generativelanguage.googleapis.com/v1beta",
            "http://192.168.1.9:11434/v1",
            "http://ollama.internal:11434/v1",
            // Not loopback just because the string appears somewhere in the host.
            "https://127.0.0.1.example.com/v1",
            "",
        ] {
            assert!(!is_loopback_url(url), "should not be loopback: {url}");
        }
    }

    #[test]
    fn a_remote_provider_needs_an_api_key() {
        let openai = AppConfig {
            active_provider: "openai".to_string(),
            providers: vec![provider("openai", "https://api.openai.com/v1", "", "gpt-4o-mini")],
            ..AppConfig::default()
        };

        let error = validate_active_provider(&openai).expect_err("a key is required");
        assert!(error.contains("API Key"), "got: {error}");
    }

    #[test]
    fn every_provider_needs_a_base_url_and_all_but_deeplx_need_a_model() {
        for name in ["openai", "ollama", "gemini", "claude"] {
            let no_model = AppConfig {
                active_provider: name.to_string(),
                providers: vec![provider(name, "http://127.0.0.1:1/v1", "", "")],
                ..AppConfig::default()
            };
            let error = validate_active_provider(&no_model).expect_err("a model is required");
            assert!(error.contains("模型"), "{name} gave: {error}");
        }

        let deeplx = AppConfig {
            active_provider: "deeplx".to_string(),
            providers: vec![provider("deeplx", "http://127.0.0.1:1188", "", "")],
            ..AppConfig::default()
        };
        assert!(validate_active_provider(&deeplx).is_ok());

        let no_url = AppConfig {
            active_provider: "ollama".to_string(),
            providers: vec![provider("ollama", "", "", "m")],
            ..AppConfig::default()
        };
        let error = validate_active_provider(&no_url).expect_err("a base URL is required");
        assert!(error.contains("Base URL"), "got: {error}");
    }

    /// `custom_http` was removed. A config still naming it must be reported, not
    /// quietly treated as ready or silently rerouted to a different service.
    #[test]
    fn a_removed_provider_is_reported_as_unknown() {
        let config = AppConfig {
            active_provider: "custom_http".to_string(),
            providers: vec![provider("custom_http", "http://127.0.0.1:8080/translate", "", "")],
            ..AppConfig::default()
        };

        let error = validate_active_provider(&config).expect_err("custom_http no longer exists");
        assert!(error.contains("未知的翻译服务"), "got: {error}");
        assert!(
            error.contains("openai") && error.contains("deeplx"),
            "the message should name what is supported: {error}"
        );
    }

    /// Every provider the defaults ship must actually be usable, or the app
    /// starts up with a provider it cannot build.
    #[test]
    fn the_default_config_only_lists_supported_providers() {
        for provider in AppConfig::default().providers {
            assert!(
                crate::translate::SUPPORTED_PROVIDERS.contains(&provider.name.as_str()),
                "{} is shipped but not supported",
                provider.name
            );
        }
    }

    /// A config on disk is the user's only copy of their API keys, so writes and
    /// reads around it are worth testing against a real filesystem rather than a
    /// string.
    mod persistence {
        use super::*;
        use std::fs;
        use std::path::PathBuf;

        /// A directory removed when the guard drops, so failures do not litter.
        struct TempDir(PathBuf);

        impl TempDir {
            fn new(label: &str) -> Self {
                let dir = std::env::temp_dir().join(format!(
                    "fk-trans-test-{label}-{}",
                    uuid::Uuid::new_v4()
                ));
                fs::create_dir_all(&dir).expect("create temp dir");
                Self(dir)
            }

            fn path(&self) -> PathBuf {
                self.0.join("config.json")
            }
        }

        impl Drop for TempDir {
            fn drop(&mut self) {
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    // Undo any read-only directory a test left behind, or the
                    // cleanup cannot remove it.
                    let _ = fs::set_permissions(&self.0, fs::Permissions::from_mode(0o700));
                }
                let _ = fs::remove_dir_all(&self.0);
            }
        }

        fn config_with_key(key: &str) -> AppConfig {
            let mut config = AppConfig::default();
            config.providers[0].api_key = key.to_string();
            config
        }

        /// The regression this guards: a config naming a provider that no longer
        /// exists must be reported, not silently dropped and not silently
        /// rerouted to a different service.
        #[test]
        fn a_removed_provider_entry_is_dropped_with_a_notice() {
            let dir = TempDir::new("removed-provider");
            let path = dir.path();
            fs::write(
                &path,
                r#"{
                    "version": 2,
                    "enabled": true,
                    "sourceLang": "auto",
                    "targetLang": "zh",
                    "activeProvider": "openai",
                    "providers": [
                        { "name": "openai", "baseUrl": "https://api.openai.com/v1", "apiKey": "sk-a", "model": "m" },
                        { "name": "custom_http", "baseUrl": "http://127.0.0.1:8080/t", "apiKey": "", "model": "" }
                    ]
                }"#,
            )
            .expect("write");

            let loaded = load_config_from(&path);

            assert!(loaded.problem.is_none());
            assert_eq!(
                loaded.config.providers.len(),
                1,
                "custom_http should have been dropped"
            );
            assert_eq!(loaded.config.providers[0].name, "openai");
            assert_eq!(loaded.notices.len(), 1);
            assert!(
                loaded.notices[0].contains("custom_http"),
                "the notice must name what was removed: {}",
                loaded.notices[0]
            );
        }

        /// Ollama is reached over its OpenAI-compatible endpoint now, so an old
        /// native base URL would be asked for a path that does not exist.
        #[test]
        fn an_ollama_base_url_gains_the_v1_path() {
            let dir = TempDir::new("ollama-url");
            let path = dir.path();
            fs::write(
                &path,
                r#"{
                    "version": 1,
                    "enabled": true,
                    "source_lang": "auto",
                    "target_lang": "zh",
                    "active_provider": "ollama",
                    "providers": [
                        { "name": "ollama", "base_url": "http://127.0.0.1:11434", "api_key": "", "model": "llama3" }
                    ]
                }"#,
            )
            .expect("write");

            let loaded = load_config_from(&path);

            assert_eq!(
                loaded.config.providers[0].base_url,
                "http://127.0.0.1:11434/v1"
            );
            assert!(
                loaded.notices.iter().any(|notice| notice.contains("/v1")),
                "the change must be reported: {:?}",
                loaded.notices
            );
        }

        #[test]
        fn an_ollama_base_url_already_pointing_at_v1_is_left_alone() {
            let dir = TempDir::new("ollama-url-current");
            let path = dir.path();
            fs::write(
                &path,
                r#"{
                    "version": 2,
                    "enabled": true,
                    "sourceLang": "auto",
                    "targetLang": "zh",
                    "activeProvider": "ollama",
                    "providers": [
                        { "name": "ollama", "baseUrl": "http://192.168.1.9:11434/v1/", "apiKey": "", "model": "m" }
                    ]
                }"#,
            )
            .expect("write");

            let loaded = load_config_from(&path);

            assert_eq!(
                loaded.config.providers[0].base_url,
                "http://192.168.1.9:11434/v1/"
            );
            assert!(
                loaded.notices.is_empty(),
                "nothing changed, so nothing to report: {:?}",
                loaded.notices
            );
        }

        #[test]
        fn a_current_config_produces_no_notices() {
            let dir = TempDir::new("no-notices");
            let path = dir.path();
            save_config_to(&path, &AppConfig::default()).expect("save");

            let loaded = load_config_from(&path);

            assert!(loaded.problem.is_none());
            assert!(
                loaded.notices.is_empty(),
                "unexpected notices: {:?}",
                loaded.notices
            );
        }

        /// A `null` extra_params parses fine but the merge step skips it, which
        /// silently dropped the seeded defaults for anything created that way.
        #[test]
        fn a_null_extra_params_becomes_an_empty_object() {
            let dir = TempDir::new("null-extra-params");
            let path = dir.path();
            fs::write(
                &path,
                r#"{
                    "version": 2,
                    "enabled": true,
                    "sourceLang": "auto",
                    "targetLang": "zh",
                    "activeProvider": "openai",
                    "providers": [
                        { "name": "openai", "baseUrl": "https://api.openai.com/v1", "apiKey": "sk-a", "model": "m", "extraParams": null }
                    ]
                }"#,
            )
            .expect("write");

            let loaded = load_config_from(&path);

            assert_eq!(
                loaded.config.providers[0].extra_params,
                serde_json::json!({}),
                "null must be normalised, not left for the merge step to skip"
            );
        }

        #[test]
        fn a_saved_config_loads_back_unchanged() {
            let dir = TempDir::new("round-trip");
            let path = dir.path();
            let config = config_with_key("sk-round-trip");

            save_config_to(&path, &config).expect("save");
            let loaded = load_config_from(&path);

            assert!(loaded.problem.is_none());
            assert_eq!(loaded.config.providers[0].api_key, "sk-round-trip");
            assert_eq!(loaded.config.target_lang, config.target_lang);
        }

        /// The file holds API keys in plain text. A default umask would leave it
        /// readable by every other account on the machine.
        #[cfg(unix)]
        #[test]
        fn a_saved_config_is_readable_only_by_its_owner() {
            use std::os::unix::fs::PermissionsExt;

            let dir = TempDir::new("permissions");
            let path = dir.path();
            save_config_to(&path, &config_with_key("sk-secret")).expect("save");

            let mode = fs::metadata(&path).expect("metadata").permissions().mode() & 0o777;
            assert_eq!(mode, 0o600, "config file mode was {mode:o}");
        }

        /// Configs written by earlier versions were world-readable. Loading one
        /// has to repair it, otherwise the existing installs stay exposed.
        #[cfg(unix)]
        #[test]
        fn loading_tightens_an_existing_world_readable_config() {
            use std::os::unix::fs::PermissionsExt;

            let dir = TempDir::new("tighten");
            let path = dir.path();
            save_config_to(&path, &config_with_key("sk-secret")).expect("save");
            fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).expect("loosen");

            let loaded = load_config_from(&path);

            assert!(loaded.problem.is_none());
            let mode = fs::metadata(&path).expect("metadata").permissions().mode() & 0o777;
            assert_eq!(mode, 0o600, "existing config should have been tightened");
        }

        /// The regression: a config that fails to parse used to be replaced by
        /// defaults with no log line at all, so the user's keys vanished on the
        /// next save and nothing said why.
        #[test]
        fn a_malformed_config_is_reported_and_preserved() {
            let dir = TempDir::new("malformed");
            let path = dir.path();
            fs::write(&path, b"{ this is not json").expect("write");

            let loaded = load_config_from(&path);

            let problem = loaded.problem.expect("a malformed config must be reported");
            assert_eq!(problem.kind, ConfigLoadProblemKind::Malformed);
            assert_eq!(problem.path, path.display().to_string());

            let backup = problem.backup_path.expect("the original must be kept");
            assert_eq!(
                fs::read_to_string(&backup).expect("backup readable"),
                "{ this is not json",
                "the unreadable config must survive in case it can be repaired by hand"
            );
            assert!(
                !path.exists(),
                "the unreadable file is moved aside so a later save cannot overwrite it"
            );
        }

        /// A config truncated mid-write by a crash parses as neither valid JSON
        /// nor valid config; it must be reported rather than discarded.
        #[test]
        fn a_truncated_config_is_reported_as_malformed() {
            let dir = TempDir::new("truncated");
            let path = dir.path();
            let mut config = AppConfig::default();
            config.providers[0].api_key = "sk-important".to_string();
            save_config_to(&path, &config).expect("save");

            let complete = fs::read_to_string(&path).expect("read");
            fs::write(&path, &complete[..complete.len() / 2]).expect("truncate");

            let loaded = load_config_from(&path);

            assert_eq!(
                loaded.problem.map(|problem| problem.kind),
                Some(ConfigLoadProblemKind::Malformed)
            );
        }

        /// The wire and on-disk format is camelCase. Files written by earlier
        /// versions are snake_case, and config.json is the only copy of the
        /// user's API keys — a rename that silently failed to read them would be
        /// data loss, not a cosmetic change.
        #[test]
        fn a_config_written_in_the_old_snake_case_format_still_loads() {
            let dir = TempDir::new("legacy-keys");
            let path = dir.path();
            fs::write(
                &path,
                r#"{
                    "enabled": true,
                    "debug_logging": false,
                    "selection_trigger_enabled": true,
                    "source_lang": "auto",
                    "target_lang": "zh",
                    "active_provider": "openai",
                    "mouse_trigger_button": 2,
                    "providers": [{
                        "name": "openai",
                        "base_url": "https://example.test/v1",
                        "api_key": "sk-legacy",
                        "model": "gpt-4o-mini",
                        "system_prompt": "sys",
                        "user_prompt": "{text}",
                        "extra_params": {}
                    }]
                }"#,
            )
            .expect("write");

            let loaded = load_config_from(&path);

            assert!(loaded.problem.is_none(), "a legacy file is not a problem");
            assert_eq!(loaded.config.providers[0].api_key, "sk-legacy");
            assert_eq!(loaded.config.providers[0].base_url, "https://example.test/v1");
            assert_eq!(loaded.config.active_provider, "openai");
        }

        /// And a file already in the current format loads without going through
        /// the alias path.
        #[test]
        fn a_config_in_the_current_format_loads() {
            let dir = TempDir::new("current-format");
            let path = dir.path();
            fs::write(
                &path,
                r#"{
                    "enabled": true,
                    "debugLogging": false,
                    "selectionTriggerEnabled": true,
                    "sourceLang": "auto",
                    "targetLang": "ja",
                    "activeProvider": "claude",
                    "mouseTriggerButton": 2,
                    "providers": [{
                        "name": "claude",
                        "baseUrl": "https://api.anthropic.com",
                        "apiKey": "sk-current",
                        "model": "claude-haiku-4-5",
                        "systemPrompt": "sys",
                        "userPrompt": "{text}",
                        "extraParams": {}
                    }]
                }"#,
            )
            .expect("write");

            let loaded = load_config_from(&path);

            assert!(loaded.problem.is_none());
            assert_eq!(loaded.config.providers[0].api_key, "sk-current");
            assert_eq!(loaded.config.target_lang, "ja");
            assert_eq!(loaded.config.active_provider, "claude");
        }

        #[test]
        fn a_missing_config_is_a_first_run_not_a_problem() {
            let dir = TempDir::new("missing");
            let loaded = load_config_from(&dir.path());

            assert!(loaded.problem.is_none());
            assert!(loaded.config.enabled);
        }

        /// The temp file used during the write must not be left behind, or the
        /// config directory fills up with copies of the API keys.
        #[test]
        fn saving_leaves_no_partial_files_behind() {
            let dir = TempDir::new("no-leftovers");
            let path = dir.path();
            save_config_to(&path, &config_with_key("sk-a")).expect("first save");
            save_config_to(&path, &config_with_key("sk-b")).expect("second save");

            let names: Vec<String> = fs::read_dir(&dir.0)
                .expect("read dir")
                .map(|entry| entry.expect("entry").file_name().to_string_lossy().into_owned())
                .collect();

            assert_eq!(names, vec!["config.json".to_string()], "found {names:?}");
            assert_eq!(
                load_config_from(&path).config.providers[0].api_key,
                "sk-b"
            );
        }

        /// A failed write must leave the previous config usable. The write goes
        /// through a sibling temp file, so a failure to create it cannot have
        /// touched the real one.
        #[cfg(unix)]
        #[test]
        fn a_failed_save_leaves_the_previous_config_intact() {
            use std::os::unix::fs::PermissionsExt;

            let dir = TempDir::new("failed-save");
            let path = dir.path();
            save_config_to(&path, &config_with_key("sk-keep-me")).expect("initial save");

            // Make the directory read-only so the temp file cannot be created.
            fs::set_permissions(&dir.0, fs::Permissions::from_mode(0o500))
                .expect("make read-only");
            let result = save_config_to(&path, &config_with_key("sk-never-written"));
            fs::set_permissions(&dir.0, fs::Permissions::from_mode(0o700))
                .expect("restore permissions");

            assert!(result.is_err(), "saving into a read-only dir must fail");
            assert_eq!(
                load_config_from(&path).config.providers[0].api_key,
                "sk-keep-me",
                "the previous config must still be there after a failed save"
            );
        }
    }

    /// `update_config` used to take a `serde_json::Value` and look keys up by
    /// hand, so a misspelled key was a silent no-op that still reported success.
    mod patch {
        use super::*;

        fn base() -> AppConfig {
            AppConfig::default()
        }

        #[test]
        fn an_empty_patch_leaves_the_config_alone() {
            let mut config = base();
            let before = serde_json::to_string(&config).unwrap();

            ConfigPatch::default().apply_to(&mut config).expect("apply");

            assert_eq!(serde_json::to_string(&config).unwrap(), before);
        }

        #[test]
        fn only_the_fields_present_are_changed() {
            let mut config = base();
            config.source_lang = "auto".to_string();
            config.target_lang = "zh".to_string();

            ConfigPatch {
                target_lang: Some("ja".to_string()),
                ..ConfigPatch::default()
            }
            .apply_to(&mut config)
            .expect("apply");

            assert_eq!(config.target_lang, "ja");
            assert_eq!(config.source_lang, "auto", "untouched field must survive");
        }

        #[test]
        fn language_codes_are_trimmed_and_must_not_be_empty() {
            let mut config = base();

            ConfigPatch {
                source_lang: Some("  en  ".to_string()),
                ..ConfigPatch::default()
            }
            .apply_to(&mut config)
            .expect("apply");
            assert_eq!(config.source_lang, "en");

            let error = ConfigPatch {
                target_lang: Some("   ".to_string()),
                ..ConfigPatch::default()
            }
            .apply_to(&mut config)
            .expect_err("a blank language must be refused");
            assert!(error.contains("目标语言"), "got: {error}");
        }

        /// A trigger button that no mouse produces used to be accepted and then
        /// simply never matched, leaving the user with an app that did nothing.
        #[test]
        fn an_out_of_range_trigger_button_is_refused() {
            let mut config = base();

            for button in [0, -1, MAX_TRIGGER_BUTTON + 1, 99] {
                let error = ConfigPatch {
                    mouse_trigger_button: Some(button),
                    ..ConfigPatch::default()
                }
                .apply_to(&mut config)
                .expect_err("should be refused");
                assert!(
                    error.contains("触发键"),
                    "button {button} gave: {error}"
                );
                assert_eq!(config.mouse_trigger_button, 2, "config must be untouched");
            }
        }

        /// Button 0 is the left button; binding it would make every click in
        /// every app fire a translation.
        #[test]
        fn the_left_button_cannot_be_bound() {
            let mut config = base();
            let error = ConfigPatch {
                mouse_trigger_button: Some(0),
                ..ConfigPatch::default()
            }
            .apply_to(&mut config)
            .expect_err("refused");
            assert!(error.contains("左键"), "got: {error}");
        }

        #[test]
        fn a_provider_that_does_not_exist_is_refused() {
            let mut config = base();

            let error = ConfigPatch {
                active_provider: Some("nope".to_string()),
                ..ConfigPatch::default()
            }
            .apply_to(&mut config)
            .expect_err("refused");

            assert!(error.contains("未知的翻译服务"), "got: {error}");
            assert_eq!(config.active_provider, "openai", "unchanged");
        }

        #[test]
        fn a_known_provider_is_accepted() {
            let mut config = base();
            ConfigPatch {
                active_provider: Some("claude".to_string()),
                ..ConfigPatch::default()
            }
            .apply_to(&mut config)
            .expect("apply");
            assert_eq!(config.active_provider, "claude");
        }

        /// The whole point of the typed patch: a key that is not a field is an
        /// error, not something quietly dropped.
        #[test]
        fn an_unknown_key_is_rejected() {
            let error = serde_json::from_str::<ConfigPatch>(r#"{ "source_language": "ja" }"#)
                .expect_err("unknown keys must not deserialize");
            assert!(
                error.to_string().contains("source_language"),
                "got: {error}"
            );
        }

        /// And the old snake_case spelling is not a field any more, on the IPC
        /// path or anywhere else.
        #[test]
        fn legacy_snake_case_keys_are_rejected_on_the_wire() {
            let error = serde_json::from_str::<ConfigPatch>(r#"{ "source_lang": "ja" }"#)
                .expect_err("snake_case must not deserialize");
            assert!(error.to_string().contains("source_lang"), "got: {error}");
        }

        #[test]
        fn a_partial_patch_deserializes_from_camel_case_json() {
            let patch: ConfigPatch = serde_json::from_str(
                r#"{ "debugLogging": true, "mouseTriggerButton": 3 }"#,
            )
            .expect("deserialize");

            assert_eq!(patch.debug_logging, Some(true));
            assert_eq!(patch.mouse_trigger_button, Some(3));
            assert_eq!(patch.target_lang, None);
        }
    }
}
