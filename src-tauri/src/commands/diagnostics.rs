use crate::config::{self, AppConfig};
use crate::mouse::listener::{self, MouseTriggerState};
use crate::ocr::OcrDiagnostic;
use crate::AppState;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use tauri::{AppHandle, Emitter, Manager, State};

const MAX_LOG_LINES: usize = 240;
const MAX_FRONTEND_LOG_CHARS: usize = 2000;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderDiagnostic {
    pub name: String,
    pub active: bool,
    pub ready: bool,
    pub reason: Option<String>,
    pub base_url_configured: bool,
    pub api_key_configured: bool,
    pub model_configured: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiagnosticsSnapshot {
    pub app_version: String,
    pub log_dir: Option<String>,
    pub debug_logging: bool,
    pub log_max_file_size_bytes: u64,
    pub log_rotation_keep_files: usize,
    pub accessibility_trusted: bool,
    pub mouse: MouseTriggerState,
    pub ocr: OcrDiagnostic,
    pub active_provider_ready: bool,
    pub active_provider_reason: Option<String>,
    pub providers: Vec<ProviderDiagnostic>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExportedDiagnostics {
    pub path: String,
}

fn provider_diagnostics(config: &AppConfig) -> Vec<ProviderDiagnostic> {
    config
        .providers
        .iter()
        .map(|provider| {
            let readiness = config::validate_provider(provider);
            ProviderDiagnostic {
                name: provider.name.clone(),
                active: provider.name == config.active_provider,
                ready: readiness.is_ok(),
                reason: readiness.err(),
                base_url_configured: !provider.base_url.trim().is_empty(),
                api_key_configured: !provider.api_key.trim().is_empty(),
                model_configured: !provider.model.trim().is_empty(),
            }
        })
        .collect()
}

fn app_log_dir(app: &AppHandle) -> Option<PathBuf> {
    app.path().app_log_dir().ok()
}

fn sanitize_line(line: &str) -> String {
    if line.contains("[clipboard] Captured:") {
        return "[clipboard] Captured: [redacted]".to_string();
    }
    if line.contains("[ocr] Recognized") {
        return "[ocr] Recognized: [redacted]".to_string();
    }
    if line.contains("Translation error:") {
        return "[pipeline] Translation error: [redacted]".to_string();
    }
    if line.to_ascii_lowercase().contains("authorization") {
        return "[redacted authorization header]".to_string();
    }
    if line.len() > 4000 {
        format!(
            "{}...[truncated]",
            line.chars().take(4000).collect::<String>()
        )
    } else {
        line.to_string()
    }
}

pub fn redact_for_diagnostics(input: &str, config: &AppConfig) -> String {
    let mut output = input.to_string();
    for provider in &config.providers {
        let key = provider.api_key.trim();
        if key.len() >= 4 {
            output = output.replace(key, "[redacted-api-key]");
        }
    }

    output
        .lines()
        .map(sanitize_line)
        .collect::<Vec<_>>()
        .join("\n")
}

fn read_recent_logs(log_dir: &Path, config: &AppConfig) -> Vec<String> {
    let mut files = match std::fs::read_dir(log_dir) {
        Ok(entries) => entries
            .filter_map(Result::ok)
            .filter_map(|entry| {
                let path = entry.path();
                if path.extension().and_then(|ext| ext.to_str()) != Some("log") {
                    return None;
                }
                let modified = entry
                    .metadata()
                    .and_then(|metadata| metadata.modified())
                    .ok()?;
                Some((path, modified))
            })
            .collect::<Vec<_>>(),
        Err(_) => return Vec::new(),
    };

    files.sort_by(|a, b| b.1.cmp(&a.1));

    let mut lines = Vec::new();
    for (path, _) in files.into_iter().take(3) {
        if let Ok(contents) = std::fs::read_to_string(path) {
            let redacted = redact_for_diagnostics(&contents, config);
            lines.extend(redacted.lines().map(ToOwned::to_owned));
        }
    }

    if lines.len() > MAX_LOG_LINES {
        lines.split_off(lines.len() - MAX_LOG_LINES)
    } else {
        lines
    }
}

fn build_report(app: &AppHandle, snapshot: &DiagnosticsSnapshot, config: &AppConfig) -> String {
    let log_lines = app_log_dir(app)
        .as_deref()
        .map(|dir| read_recent_logs(dir, config))
        .unwrap_or_default();

    let provider_lines = snapshot
        .providers
        .iter()
        .map(|provider| {
            format!(
                "- {}{}: ready={}, base_url={}, api_key={}, model={}, reason={}",
                provider.name,
                if provider.active { " (active)" } else { "" },
                provider.ready,
                provider.base_url_configured,
                provider.api_key_configured,
                provider.model_configured,
                provider.reason.as_deref().unwrap_or("none")
            )
        })
        .collect::<Vec<_>>()
        .join("\n");

    let report = format!(
        r#"fk-trans diagnostics
generated_at: {}
app_version: {}
log_dir: {}
debug_logging: {}
log_max_file_size_bytes: {}
log_rotation_keep_files: {}

mouse_trigger:
  status: {:?}
  accessibility_trusted: {}
  trigger_button: {}
  last_button: {:?}
  last_event_at: {:?}
  last_trigger_at: {:?}
  last_pipeline_at: {:?}
  last_pipeline_source: {:?}
  last_pipeline_result: {:?}
  last_error: {:?}
  test_active_until: {:?}

provider_readiness:
  active_provider_ready: {}
  active_provider_reason: {}
{}

ocr:
  enabled: {}
  backend: {}
  ready: {}
  reason: {}
  screen_capture_ready: {}
  last_result: {}
  last_error: {}
  last_elapsed_ms: {}

recent_logs:
{}
"#,
        chrono::Utc::now().to_rfc3339(),
        snapshot.app_version,
        snapshot.log_dir.as_deref().unwrap_or("unknown"),
        snapshot.debug_logging,
        snapshot.log_max_file_size_bytes,
        snapshot.log_rotation_keep_files,
        snapshot.mouse.status,
        snapshot.mouse.accessibility_trusted,
        snapshot.mouse.trigger_button,
        snapshot.mouse.last_button,
        snapshot.mouse.last_event_at,
        snapshot.mouse.last_trigger_at,
        snapshot.mouse.last_pipeline_at,
        snapshot.mouse.last_pipeline_source,
        snapshot.mouse.last_pipeline_result,
        snapshot.mouse.last_error,
        snapshot.mouse.test_active_until,
        snapshot.active_provider_ready,
        snapshot.active_provider_reason.as_deref().unwrap_or("none"),
        provider_lines,
        snapshot.ocr.enabled,
        snapshot.ocr.backend,
        snapshot.ocr.ready,
        snapshot.ocr.reason.as_deref().unwrap_or("none"),
        snapshot.ocr.screen_capture_ready,
        snapshot.ocr.last_result.as_deref().unwrap_or("none"),
        snapshot.ocr.last_error.as_deref().unwrap_or("none"),
        snapshot
            .ocr
            .last_elapsed_ms
            .map(|value| value.to_string())
            .unwrap_or_else(|| "none".to_string()),
        if log_lines.is_empty() {
            "(no log lines found)".to_string()
        } else {
            log_lines.join("\n")
        }
    );

    redact_for_diagnostics(&report, config)
}

fn current_snapshot(app: &AppHandle, state: &AppState) -> DiagnosticsSnapshot {
    let config = state
        .config
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clone();
    let mut mouse = listener::snapshot(&state.mouse_trigger_state);

    #[cfg(target_os = "macos")]
    {
        mouse.accessibility_trusted = crate::platform::macos::check_accessibility_permissions();
        if !mouse.accessibility_trusted
            && !matches!(
                mouse.status,
                listener::MouseTriggerStatus::Failed
                    | listener::MouseTriggerStatus::PermissionMissing
            )
        {
            mouse.status = listener::MouseTriggerStatus::PermissionMissing;
        }
    }

    let active_readiness = config::validate_active_provider(&config);

    DiagnosticsSnapshot {
        app_version: app.package_info().version.to_string(),
        log_dir: app_log_dir(app).map(|path| path.display().to_string()),
        debug_logging: config.debug_logging,
        log_max_file_size_bytes: crate::LOG_MAX_FILE_SIZE_BYTES,
        log_rotation_keep_files: crate::LOG_ROTATION_KEEP_FILES,
        accessibility_trusted: mouse.accessibility_trusted,
        mouse,
        ocr: state.ocr_runtime.snapshot(config.ocr_enabled),
        active_provider_ready: active_readiness.is_ok(),
        active_provider_reason: active_readiness.err(),
        providers: provider_diagnostics(&config),
    }
}

#[tauri::command]
pub fn get_diagnostics_snapshot(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<DiagnosticsSnapshot, String> {
    Ok(current_snapshot(&app, &state))
}

#[tauri::command]
pub fn start_middle_click_test(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<MouseTriggerState, String> {
    let snapshot = listener::start_test_window(&state.mouse_trigger_state, 10_000);
    let _ = app.emit("mouse-trigger-state", snapshot.clone());
    Ok(snapshot)
}

#[tauri::command]
pub fn export_diagnostics_report(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<ExportedDiagnostics, String> {
    let config = state
        .config
        .lock()
        .map_err(|_| "Config lock poisoned".to_string())?
        .clone();
    let snapshot = current_snapshot(&app, &state);
    let log_dir = app_log_dir(&app).ok_or_else(|| "Unable to resolve log directory".to_string())?;
    std::fs::create_dir_all(&log_dir)
        .map_err(|e| format!("Failed to create diagnostics directory: {}", e))?;

    let file_name = format!(
        "fk-trans-diagnostics-{}.txt",
        chrono::Local::now().format("%Y%m%d-%H%M%S")
    );
    let path = log_dir.join(file_name);
    let report = build_report(&app, &snapshot, &config);
    std::fs::write(&path, report)
        .map_err(|e| format!("Failed to write diagnostics report: {}", e))?;
    Ok(ExportedDiagnostics {
        path: path.display().to_string(),
    })
}

#[tauri::command]
pub fn reveal_diagnostics_folder(app: AppHandle) -> Result<(), String> {
    let log_dir = app_log_dir(&app).ok_or_else(|| "Unable to resolve log directory".to_string())?;
    std::fs::create_dir_all(&log_dir)
        .map_err(|e| format!("Failed to create log directory: {}", e))?;
    open_path(&log_dir)
}

#[tauri::command]
pub fn open_accessibility_settings() -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        crate::platform::macos::open_accessibility_settings()
    }
    #[cfg(not(target_os = "macos"))]
    {
        Err("Accessibility settings shortcut is only available on macOS".to_string())
    }
}

#[tauri::command]
pub fn log_frontend_event(
    level: String,
    message: String,
    context: Option<serde_json::Value>,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let config = state
        .config
        .lock()
        .map_err(|_| "Config lock poisoned".to_string())?
        .clone();
    let context = context
        .map(|value| value.to_string())
        .unwrap_or_else(|| "{}".to_string());
    let raw = format!(
        "{} {}",
        message
            .chars()
            .take(MAX_FRONTEND_LOG_CHARS)
            .collect::<String>(),
        context
            .chars()
            .take(MAX_FRONTEND_LOG_CHARS)
            .collect::<String>()
    );
    let redacted = redact_for_diagnostics(&raw, &config);

    match level.as_str() {
        "error" => log::error!(target: "webview", "[frontend] {}", redacted),
        "warn" => log::warn!(target: "webview", "[frontend] {}", redacted),
        _ => log::info!(target: "webview", "[frontend] {}", redacted),
    }
    Ok(())
}

fn open_path(path: &Path) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg(path)
            .spawn()
            .map_err(|e| format!("Failed to open path: {}", e))?;
        Ok(())
    }
    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("explorer")
            .arg(path)
            .spawn()
            .map_err(|e| format!("Failed to open path: {}", e))?;
        Ok(())
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        std::process::Command::new("xdg-open")
            .arg(path)
            .spawn()
            .map_err(|e| format!("Failed to open path: {}", e))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diagnostics_redaction_removes_api_keys_and_clipboard_content() {
        let config = AppConfig {
            providers: vec![crate::config::ProviderConfig {
                name: "openai".to_string(),
                base_url: "https://api.example.com/v1".to_string(),
                api_key: "sk-secret-value".to_string(),
                model: "model".to_string(),
                system_prompt: config::default_system_prompt(),
                user_prompt: config::default_user_prompt(),
                extra_params: serde_json::json!({}),
            }],
            ..AppConfig::default()
        };
        let raw = r#"
Authorization: Bearer sk-secret-value
[clipboard] Captured: "private selected text"
provider error sk-secret-value
"#;

        let redacted = redact_for_diagnostics(raw, &config);

        assert!(!redacted.contains("sk-secret-value"));
        assert!(!redacted.contains("private selected text"));
        assert!(redacted.contains("[redacted-api-key]"));
    }
}
