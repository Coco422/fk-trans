use serde::{Deserialize, Serialize};
use std::io::Write;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderConfig {
    pub name: String,
    pub base_url: String,
    pub api_key: String,
    pub model: String,
    #[serde(default = "default_system_prompt")]
    pub system_prompt: String,
    #[serde(default = "default_user_prompt")]
    pub user_prompt: String,
    #[serde(default)]
    pub extra_params: serde_json::Value,
}

pub fn default_system_prompt() -> String {
    "You are a translator. Translate the following text from {from} to {to}. Output ONLY the translation, nothing else.".to_string()
}

pub fn default_user_prompt() -> String {
    "{text}".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub enabled: bool,
    #[serde(default)]
    pub debug_logging: bool,
    #[serde(default = "default_selection_trigger_enabled")]
    pub selection_trigger_enabled: bool,
    pub source_lang: String,
    pub target_lang: String,
    pub active_provider: String,
    #[serde(default = "default_mouse_trigger_button")]
    pub mouse_trigger_button: i64,
    pub providers: Vec<ProviderConfig>,
}

pub fn default_mouse_trigger_button() -> i64 {
    2
}

pub fn default_selection_trigger_enabled() -> bool {
    true
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
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
                    base_url: "http://127.0.0.1:11434".into(),
                    api_key: String::new(),
                    model: "llama3".into(),
                    system_prompt: default_system_prompt(),
                    user_prompt: default_user_prompt(),
                    extra_params: serde_json::json!({}),
                },
                ProviderConfig {
                    name: "custom_http".into(),
                    base_url: String::new(),
                    api_key: String::new(),
                    model: String::new(),
                    system_prompt: String::new(),
                    user_prompt: String::new(),
                    extra_params: serde_json::json!({}),
                },
            ],
        }
    }
}

fn is_blank(value: &str) -> bool {
    value.trim().is_empty()
}

pub fn validate_provider(provider: &ProviderConfig) -> Result<(), String> {
    match provider.name.as_str() {
        "deeplx" => {
            if is_blank(&provider.base_url) {
                Err("DeepLX base URL is not configured".to_string())
            } else {
                Ok(())
            }
        }
        "ollama" => {
            if is_blank(&provider.base_url) {
                Err("Ollama base URL is not configured".to_string())
            } else if is_blank(&provider.model) {
                Err("Ollama model is not configured".to_string())
            } else {
                Ok(())
            }
        }
        "custom_http" => {
            if is_blank(&provider.base_url) {
                Err("Custom HTTP endpoint is not configured".to_string())
            } else {
                Ok(())
            }
        }
        "openai" | "gemini" | "claude" => {
            if is_blank(&provider.base_url) {
                Err(format!("{} base URL is not configured", provider.name))
            } else if is_blank(&provider.api_key) {
                Err(format!("{} API key is not configured", provider.name))
            } else if is_blank(&provider.model) {
                Err(format!("{} model is not configured", provider.name))
            } else {
                Ok(())
            }
        }
        _ => Err(format!("Unknown provider: {}", provider.name)),
    }
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
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigLoadProblem {
    pub path: String,
    pub kind: ConfigLoadProblemKind,
    pub detail: String,
    /// Where the unreadable file was preserved, when it could be moved aside.
    pub backup_path: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
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
}

fn config_dir() -> Result<PathBuf, String> {
    dirs::config_dir()
        .map(|dir| dir.join("fk-trans"))
        .ok_or_else(|| "Could not locate the user config directory".to_string())
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
            };
        }
    };

    match serde_json::from_str::<AppConfig>(&json) {
        Ok(config) => {
            tighten_config_permissions(path);
            ConfigLoad {
                config,
                problem: None,
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
            }
        }
    }
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

    #[test]
    fn legacy_config_without_mouse_trigger_button_uses_default() {
        let json = r#"{
            "enabled": true,
            "source_lang": "auto",
            "target_lang": "zh",
            "active_provider": "openai",
            "providers": []
        }"#;

        let config: AppConfig = serde_json::from_str(json).unwrap();

        assert_eq!(config.mouse_trigger_button, 2);
        assert!(!config.debug_logging);
        assert!(config.selection_trigger_enabled);
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

    #[test]
    fn local_providers_are_ready_with_local_endpoint_requirements() {
        let deeplx = AppConfig {
            active_provider: "deeplx".to_string(),
            providers: vec![provider("deeplx", "http://127.0.0.1:1188", "", "")],
            ..AppConfig::default()
        };
        let ollama = AppConfig {
            active_provider: "ollama".to_string(),
            providers: vec![provider("ollama", "http://127.0.0.1:11434", "", "llama3")],
            ..AppConfig::default()
        };
        let custom_http = AppConfig {
            active_provider: "custom_http".to_string(),
            providers: vec![provider(
                "custom_http",
                "http://127.0.0.1:8080/translate",
                "",
                "",
            )],
            ..AppConfig::default()
        };

        assert!(validate_active_provider(&deeplx).is_ok());
        assert!(validate_active_provider(&ollama).is_ok());
        assert!(validate_active_provider(&custom_http).is_ok());
    }

    #[test]
    fn local_providers_reject_missing_required_fields() {
        let ollama = AppConfig {
            active_provider: "ollama".to_string(),
            providers: vec![provider("ollama", "http://127.0.0.1:11434", "", "")],
            ..AppConfig::default()
        };
        let custom_http = AppConfig {
            active_provider: "custom_http".to_string(),
            providers: vec![provider("custom_http", "", "", "")],
            ..AppConfig::default()
        };

        assert!(validate_active_provider(&ollama).is_err());
        assert!(validate_active_provider(&custom_http).is_err());
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
}
