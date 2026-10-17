/**
 * The only place the frontend talks to Rust.
 *
 * Every command argument and return value is a generated type from
 * `./types.ts`, which ts-rs produces from the Rust structs. Nothing here is
 * hand-maintained, so a change on one side cannot silently drift from the other.
 *
 * Errors are normalised into {@link IpcError}. Rust commands return
 * `Result<_, String>`, so a rejection is usually a plain string; `String(error)`
 * on those produced reasonable text, but a rejection from the IPC layer itself
 * could be an object, which rendered as `[object Object]`.
 */
import { invoke } from "@tauri-apps/api/core";
import type {
  AppConfig,
  ConfigPatch,
  DiagnosticsSnapshot,
  ExportedDiagnostics,
  HistoryEntry,
  MouseTriggerState,
  ProviderUpdate,
  TranslateResult,
  TranslationReadyPayload,
} from "./types";

export type { TranslationReadyPayload };

/** A command failure, carrying which command failed so the UI can be specific. */
export class IpcError extends Error {
  readonly command: string;

  constructor(command: string, message: string) {
    super(message);
    this.name = "IpcError";
    this.command = command;
  }
}

function describe(error: unknown): string {
  if (typeof error === "string") {
    return error;
  }
  if (error instanceof Error) {
    return error.message;
  }
  try {
    const encoded = JSON.stringify(error);
    if (encoded && encoded !== "{}") {
      return encoded;
    }
  } catch {
    // Fall through to String().
  }
  return String(error);
}

async function call<T>(
  command: string,
  args?: Record<string, unknown>
): Promise<T> {
  try {
    return await invoke<T>(command, args);
  } catch (error) {
    throw new IpcError(command, describe(error));
  }
}

export function getConfig(): Promise<AppConfig> {
  return call<AppConfig>("get_config");
}

/**
 * Apply a partial update. The argument is a generated `ConfigPatch`; a key that
 * is not a config field is rejected by Rust rather than silently ignored.
 */
export function updateConfig(patch: ConfigPatch): Promise<AppConfig> {
  return call<AppConfig>("update_config", { patch });
}

export function updateProvider(update: ProviderUpdate): Promise<void> {
  return call<void>("update_provider", { update });
}

/** Translates a fixed probe string to prove the provider credentials work. */
export function testProvider(providerName: string): Promise<string> {
  return call<string>("test_provider", { providerName });
}

/**
 * Runs one of the popup's extra actions (explain / dict / summary / polish).
 * The prompt for each lives in Rust so it is not duplicated here.
 */
export function aiAction(args: {
  text: string;
  action: string;
  sourceLang: string;
  targetLang: string;
}): Promise<string> {
  return call<string>("ai_action", args);
}

export function translateText(args: {
  text: string;
  from: string;
  to: string;
}): Promise<TranslateResult> {
  return call<TranslateResult>("translate_text", args);
}

export function getHistory(): Promise<HistoryEntry[]> {
  return call<HistoryEntry[]>("get_history");
}

export function clearHistory(): Promise<void> {
  return call<void>("clear_history");
}

export function getDiagnosticsSnapshot(): Promise<DiagnosticsSnapshot> {
  return call<DiagnosticsSnapshot>("get_diagnostics_snapshot");
}

/** Opens a 10 second window during which any mouse button press is reported. */
export function startMiddleClickTest(): Promise<MouseTriggerState> {
  return call<MouseTriggerState>("start_middle_click_test");
}

export function exportDiagnosticsReport(): Promise<ExportedDiagnostics> {
  return call<ExportedDiagnostics>("export_diagnostics_report");
}

export function revealDiagnosticsFolder(): Promise<void> {
  return call<void>("reveal_diagnostics_folder");
}

export function openAccessibilitySettings(): Promise<void> {
  return call<void>("open_accessibility_settings");
}

export function logFrontendEvent(args: {
  level: string;
  message: string;
  context?: unknown;
}): Promise<void> {
  return call<void>("log_frontend_event", args);
}

/**
 * Writes text to the clipboard through the clipboard plugin.
 *
 * `navigator.clipboard` is unavailable in the popup window, so this is the
 * supported path. Kept here rather than inline so the plugin's command name
 * appears exactly once: as a bare `invoke("plugin:clipboard-manager|write_text")`
 * it would break silently if the plugin's command path ever changed.
 */
export function writeClipboard(text: string): Promise<void> {
  return call<void>("plugin:clipboard-manager|write_text", { text });
}
