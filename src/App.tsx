import { createSignal, createResource, For, Show, onMount, onCleanup } from "solid-js";
import { getVersion } from "@tauri-apps/api/app";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { relaunch } from "@tauri-apps/plugin-process";
import { check, type DownloadEvent } from "@tauri-apps/plugin-updater";
import { buildUpdateProviderArgs } from "./tauriArgs";

interface ProviderConfig {
  name: string;
  base_url: string;
  api_key: string;
  model: string;
  system_prompt: string;
  user_prompt: string;
  extra_params: Record<string, unknown>;
}

interface AppConfig {
  enabled: boolean;
  debug_logging: boolean;
  ocr_enabled: boolean;
  source_lang: string;
  target_lang: string;
  active_provider: string;
  mouse_trigger_button: number;
  providers: ProviderConfig[];
}

interface HistoryEntry {
  id: string;
  timestamp: number;
  original: string;
  translated: string;
  source_lang: string;
  target_lang: string;
  provider: string;
}

interface UpdateStatus {
  status: "idle" | "checking" | "downloading" | "installed" | "none" | "error";
  message: string;
}

type ProviderStatus =
  | "idle"
  | "dirty"
  | "saving"
  | "saved"
  | "save_error"
  | "testing"
  | "test_success"
  | "test_error";

interface ProviderUiStatus {
  status: ProviderStatus;
  message: string;
}

interface MouseTriggerState {
  status: string;
  accessibility_trusted: boolean;
  trigger_button: number;
  last_button?: number | null;
  last_event_at?: number | null;
  last_trigger_at?: number | null;
  last_pipeline_at?: number | null;
  last_pipeline_source?: string | null;
  last_pipeline_result?: string | null;
  last_error?: string | null;
  test_active_until?: number | null;
}

interface DiagnosticsSnapshot {
  app_version: string;
  log_dir?: string | null;
  debug_logging: boolean;
  log_max_file_size_bytes: number;
  log_rotation_keep_files: number;
  accessibility_trusted: boolean;
  mouse: MouseTriggerState;
  ocr: {
    enabled: boolean;
    backend: string;
    ready: boolean;
    reason?: string | null;
    screen_capture_ready: boolean;
    last_result?: string | null;
    last_error?: string | null;
    last_elapsed_ms?: number | null;
  };
  active_provider_ready: boolean;
  active_provider_reason?: string | null;
  providers: Array<{
    name: string;
    active: boolean;
    ready: boolean;
    reason?: string | null;
    base_url_configured: boolean;
    api_key_configured: boolean;
    model_configured: boolean;
  }>;
}

interface ExportedDiagnostics {
  path: string;
}

const LANGUAGES = [
  { code: "auto", label: "Auto Detect" },
  { code: "en", label: "English" },
  { code: "zh", label: "Chinese" },
  { code: "ja", label: "Japanese" },
  { code: "ko", label: "Korean" },
  { code: "fr", label: "French" },
  { code: "de", label: "German" },
  { code: "es", label: "Spanish" },
  { code: "ru", label: "Russian" },
  { code: "pt", label: "Portuguese" },
  { code: "ar", label: "Arabic" },
];

const PROVIDER_LABELS: Record<string, string> = {
  deeplx: "DeepLX (Local)",
  openai: "OpenAI Compatible",
  gemini: "Google Gemini",
  claude: "Anthropic Claude",
  ollama: "Ollama (Local)",
  custom_http: "Custom HTTP",
};

const LLM_PROVIDERS = new Set(["openai", "gemini", "claude", "ollama"]);

export default function App() {
  const [config, { mutate }] = createResource<AppConfig>(() =>
    invoke("get_config")
  );
  const [history, setHistory] = createSignal<HistoryEntry[]>([]);
  const [providerStatus, setProviderStatus] = createSignal<
    Record<string, ProviderUiStatus>
  >({});
  const [activeTab, setActiveTab] = createSignal<
    "general" | "providers" | "diagnostics" | "history"
  >("general");
  const [expandedProvider, setExpandedProvider] = createSignal<string | null>(null);
  const [providerDrafts, setProviderDrafts] = createSignal<
    Record<string, ProviderConfig>
  >({});
  const [extraParamDrafts, setExtraParamDrafts] = createSignal<
    Record<string, string>
  >({});
  const [updateStatus, setUpdateStatus] = createSignal<UpdateStatus>({
    status: "idle",
    message: "",
  });
  const [appVersion, setAppVersion] = createSignal("");
  const [diagnostics, setDiagnostics] = createSignal<DiagnosticsSnapshot | null>(null);
  const [diagnosticsMessage, setDiagnosticsMessage] = createSignal("");

  onMount(() => {
    let unlistenConfig: (() => void) | undefined;
    let unlistenMouse: (() => void) | undefined;

    getVersion()
      .then(setAppVersion)
      .catch(() => setAppVersion(""));

    refreshDiagnostics();

    listen("config-changed", async () => {
      try {
        const updated = await invoke<AppConfig>("get_config");
        mutate(updated);
      } catch {}
    }).then((unlisten) => {
      unlistenConfig = unlisten;
    });
    listen<MouseTriggerState>("mouse-trigger-state", (event) => {
      setDiagnostics((prev) =>
        prev
          ? {
              ...prev,
              accessibility_trusted: event.payload.accessibility_trusted,
              mouse: event.payload,
            }
          : prev
      );
    }).then((unlisten) => {
      unlistenMouse = unlisten;
    });
    const interval = window.setInterval(refreshDiagnostics, 1000);
    onCleanup(() => {
      unlistenConfig?.();
      unlistenMouse?.();
      window.clearInterval(interval);
    });
  });

  async function loadHistory() {
    try {
      const entries = await invoke<HistoryEntry[]>("get_history");
      setHistory(entries);
    } catch {
      setHistory([]);
    }
  }

  async function clearHistory() {
    await invoke("clear_history");
    setHistory([]);
  }

  async function saveConfig(updates: Partial<AppConfig>) {
    const updated = await invoke<AppConfig>("update_config", { updates });
    mutate(updated);
    await refreshDiagnostics();
  }

  async function refreshDiagnostics() {
    try {
      setDiagnostics(await invoke<DiagnosticsSnapshot>("get_diagnostics_snapshot"));
    } catch {
      setDiagnostics(null);
    }
  }

  async function startMiddleClickTest() {
    setDiagnosticsMessage("Press the mouse wheel within 10 seconds.");
    try {
      const mouse = await invoke<MouseTriggerState>("start_middle_click_test");
      setDiagnostics((prev) =>
        prev
          ? {
              ...prev,
              mouse,
            }
          : prev
      );
    } catch (e) {
      setDiagnosticsMessage(`Failed to start test: ${String(e)}`);
    }
  }

  async function saveLastMouseButton() {
    const lastButton = diagnostics()?.mouse.last_button;
    if (lastButton === undefined || lastButton === null) return;
    try {
      await saveConfig({ mouse_trigger_button: lastButton });
      setDiagnosticsMessage(`Saved button ${lastButton} as the trigger.`);
    } catch (e) {
      setDiagnosticsMessage(`Failed to save trigger button: ${String(e)}`);
    }
  }

  async function openAccessibilitySettings() {
    try {
      await invoke("open_accessibility_settings");
      setDiagnosticsMessage("Opened macOS Accessibility settings.");
    } catch (e) {
      setDiagnosticsMessage(String(e));
    }
  }

  async function openScreenRecordingSettings() {
    try {
      await invoke("open_screen_recording_settings");
      setDiagnosticsMessage("Opened macOS Screen Recording settings.");
    } catch (e) {
      setDiagnosticsMessage(String(e));
    }
  }

  async function exportDiagnostics() {
    try {
      const result = await invoke<ExportedDiagnostics>("export_diagnostics_report");
      setDiagnosticsMessage(`Diagnostics exported: ${result.path}`);
    } catch (e) {
      setDiagnosticsMessage(`Export failed: ${String(e)}`);
    }
  }

  async function revealDiagnosticsFolder() {
    try {
      await invoke("reveal_diagnostics_folder");
    } catch (e) {
      setDiagnosticsMessage(`Reveal failed: ${String(e)}`);
    }
  }

  function providerDraft(provider: ProviderConfig) {
    return providerDrafts()[provider.name] ?? provider;
  }

  function updateProviderDraft(
    provider: ProviderConfig,
    field: keyof ProviderConfig,
    value: string | Record<string, unknown>
  ) {
    const nextDraft = {
      ...(providerDrafts()[provider.name] ?? provider),
      [field]: value,
    } as ProviderConfig;
    setProviderDrafts((prev) => ({
      ...prev,
      [provider.name]: nextDraft,
    }));
    setProviderStatus((prev) => ({
      ...prev,
      [provider.name]: providerChanged(nextDraft, provider)
        ? { status: "dirty", message: "Unsaved changes" }
        : { status: "idle", message: "" },
    }));
  }

  function clearProviderDraft(name: string) {
    setProviderDrafts((prev) => {
      const next = { ...prev };
      delete next[name];
      return next;
    });
  }

  function providerChanged(a: ProviderConfig, b: ProviderConfig) {
    return (
      a.base_url !== b.base_url ||
      a.api_key !== b.api_key ||
      a.model !== b.model ||
      a.system_prompt !== b.system_prompt ||
      a.user_prompt !== b.user_prompt ||
      JSON.stringify(a.extra_params ?? {}) !== JSON.stringify(b.extra_params ?? {})
    );
  }

  async function commitProvider(name: string) {
    const cfg = config();
    if (!cfg) return true;
    const provider = cfg.providers.find((p) => p.name === name);
    const draft = providerDrafts()[name];
    if (!provider || !draft) return true;

    if (!providerChanged(draft, provider)) {
      clearProviderDraft(name);
      setProviderStatus((prev) => ({
        ...prev,
        [name]: { status: "saved", message: "Saved" },
      }));
      return true;
    }

    setProviderStatus((prev) => ({
      ...prev,
      [name]: { status: "saving", message: "Saving..." },
    }));

    try {
      await invoke("update_provider", buildUpdateProviderArgs(name, draft));
      const updated = await invoke<AppConfig>("get_config");
      mutate(updated);
      clearProviderDraft(name);
      setProviderStatus((prev) => ({
        ...prev,
        [name]: { status: "saved", message: "Saved" },
      }));
      await refreshDiagnostics();
      return true;
    } catch (e) {
      setProviderStatus((prev) => ({
        ...prev,
        [name]: { status: "save_error", message: String(e) },
      }));
      return false;
    }
  }

  function extraParamsValue(provider: ProviderConfig) {
    return (
      extraParamDrafts()[provider.name] ??
      JSON.stringify(providerDraft(provider).extra_params || {}, null, 2)
    );
  }

  function updateExtraParamsDraft(provider: ProviderConfig, raw: string) {
    setExtraParamDrafts((prev) => ({ ...prev, [provider.name]: raw }));
    setProviderStatus((prev) => ({
      ...prev,
      [provider.name]: { status: "dirty", message: "Unsaved changes" },
    }));
    try {
      updateProviderDraft(provider, "extra_params", JSON.parse(raw));
    } catch {
      // Keep the raw draft so typing invalid intermediate JSON does not get clobbered.
    }
  }

  async function commitExtraParams(provider: ProviderConfig) {
    const raw = extraParamDrafts()[provider.name];
    if (raw === undefined) return commitProvider(provider.name);

    try {
      updateProviderDraft(provider, "extra_params", JSON.parse(raw));
      setExtraParamDrafts((prev) => {
        const next = { ...prev };
        delete next[provider.name];
        return next;
      });
      return commitProvider(provider.name);
    } catch {
      setProviderStatus((prev) => ({
        ...prev,
        [provider.name]: {
          status: "save_error",
          message: "Extra Parameters must be valid JSON before saving.",
        },
      }));
      return false;
    }
  }

  async function testProvider(name: string) {
    const provider = config()?.providers.find((p) => p.name === name);
    const committed = provider
      ? await commitExtraParams(provider)
      : await commitProvider(name);
    if (!committed) return;

    setProviderStatus((prev) => ({
      ...prev,
      [name]: { status: "testing", message: "Testing..." },
    }));
    try {
      const result = await invoke<string>("test_provider", {
        providerName: name,
      });
      setProviderStatus((prev) => ({
        ...prev,
        [name]: { status: "test_success", message: result },
      }));
    } catch (e) {
      setProviderStatus((prev) => ({
        ...prev,
        [name]: { status: "test_error", message: String(e) },
      }));
    }
  }

  function statusFor(provider: ProviderConfig): ProviderUiStatus {
    const explicit = providerStatus()[provider.name];
    if (explicit) return explicit;
    const draft = providerDrafts()[provider.name];
    if (draft && providerChanged(draft, provider)) {
      return { status: "dirty", message: "Unsaved changes" };
    }
    return { status: "idle", message: "" };
  }

  function providerBusy(provider: ProviderConfig) {
    const status = statusFor(provider).status;
    return status === "saving" || status === "testing";
  }

  function providerStatusTone(status: ProviderStatus) {
    if (status === "test_success" || status === "saved") return "text-green-500";
    if (status === "test_error" || status === "save_error") return "text-red-500";
    if (status === "dirty") return "text-amber-500";
    return "text-gray-400";
  }

  function providerStatusLabel(status: ProviderStatus) {
    switch (status) {
      case "dirty":
        return "Unsaved changes";
      case "saving":
        return "Saving...";
      case "saved":
        return "Saved";
      case "save_error":
        return "Save failed";
      case "testing":
        return "Testing...";
      case "test_success":
        return "Test OK";
      case "test_error":
        return "Test failed";
      default:
        return "";
    }
  }

  function formatTime(ts: number) {
    return new Date(ts * 1000).toLocaleString();
  }

  function formatMillis(ts?: number | null) {
    if (!ts) return "Never";
    return new Date(ts).toLocaleString();
  }

  async function checkForUpdates() {
    setUpdateStatus({ status: "checking", message: "Checking for updates..." });

    try {
      const update = await check();

      if (!update) {
        const version = appVersion();
        setUpdateStatus({
          status: "none",
          message: version
            ? `You are already on the latest version: v${version}.`
            : "You are already on the latest version.",
        });
        return;
      }

      let downloaded = 0;
      let total: number | undefined;

      await update.downloadAndInstall((event: DownloadEvent) => {
        if (event.event === "Started") {
          total = event.data.contentLength;
          downloaded = 0;
          setUpdateStatus({
            status: "downloading",
            message: `Downloading v${update.version}...`,
          });
        } else if (event.event === "Progress") {
          downloaded += event.data.chunkLength;
          const progress =
            total && total > 0 ? ` ${Math.round((downloaded / total) * 100)}%` : "";
          setUpdateStatus({
            status: "downloading",
            message: `Downloading v${update.version}${progress}`,
          });
        } else {
          setUpdateStatus({
            status: "installed",
            message: `Installed v${update.version}. Relaunching...`,
          });
        }
      });

      await relaunch();
    } catch (e) {
      setUpdateStatus({
        status: "error",
        message: `Update failed: ${String(e)}`,
      });
    }
  }

  return (
    <div class="h-screen flex flex-col bg-gray-50 dark:bg-gray-950 text-gray-900 dark:text-gray-100">
      {/* Fixed header */}
      <div class="flex-shrink-0 max-w-2xl mx-auto w-full px-6 pt-6 pb-0">
        <div class="flex items-center justify-between mb-4">
          <h1 class="text-xl font-semibold">fk-trans</h1>
          <span class="text-xs text-gray-400">
            {appVersion() ? `v${appVersion()}` : ""}
          </span>
        </div>

        {/* Tab bar */}
        <div class="flex gap-1 border-b border-gray-200 dark:border-gray-800">
          <For each={["general", "providers", "diagnostics", "history"] as const}>
            {(tab) => (
              <button
                class={`px-4 py-2 text-sm font-medium transition-colors cursor-pointer border-b-2 -mb-px ${
                  activeTab() === tab
                    ? "border-blue-500 text-blue-600 dark:text-blue-400"
                    : "border-transparent text-gray-500 hover:text-gray-700 dark:hover:text-gray-300"
                }`}
                onClick={() => {
                  setActiveTab(tab);
                  if (tab === "history") loadHistory();
                }}
              >
                {tab === "general"
                  ? "General"
                  : tab === "providers"
                  ? "Providers"
                  : tab === "diagnostics"
                  ? "Diagnostics"
                  : "History"}
              </button>
            )}
          </For>
        </div>
      </div>

      {/* Scrollable content */}
      <div class="flex-1 overflow-y-auto">
        <div class="max-w-2xl mx-auto w-full px-6 py-6">
          <Show when={config()}>
            {(cfg) => (
              <>
                {/* General tab */}
                <Show when={activeTab() === "general"}>
                  <div class="space-y-6">
                    {/* Enable toggle */}
                    <div class="flex items-center justify-between p-4 bg-white dark:bg-gray-900 rounded-lg border border-gray-200 dark:border-gray-800">
                      <div>
                        <div class="text-sm font-medium">Enable fk-trans</div>
                        <div class="text-xs text-gray-500">
                          Middle-click to translate selected text
                        </div>
                      </div>
                      <button
                        class={`w-11 h-6 rounded-full transition-colors cursor-pointer ${
                          cfg().enabled
                            ? "bg-blue-500"
                            : "bg-gray-300 dark:bg-gray-700"
                        }`}
                        onClick={() => saveConfig({ enabled: !cfg().enabled })}
                      >
                        <div
                          class={`w-5 h-5 rounded-full bg-white shadow-sm transition-transform ${
                            cfg().enabled
                              ? "translate-x-[22px]"
                              : "translate-x-[2px]"
                          }`}
                        />
                      </button>
                    </div>

                    {/* OCR toggle */}
                    <div class="flex items-center justify-between p-4 bg-white dark:bg-gray-900 rounded-lg border border-gray-200 dark:border-gray-800">
                      <div>
                        <div class="text-sm font-medium">OCR Shortcut</div>
                        <div class="text-xs text-gray-500">
                          Cmd+Shift+O screenshot region translation
                        </div>
                      </div>
                      <button
                        class={`w-11 h-6 rounded-full transition-colors cursor-pointer ${
                          cfg().ocr_enabled
                            ? "bg-blue-500"
                            : "bg-gray-300 dark:bg-gray-700"
                        }`}
                        onClick={() =>
                          saveConfig({ ocr_enabled: !cfg().ocr_enabled })
                        }
                      >
                        <div
                          class={`w-5 h-5 rounded-full bg-white shadow-sm transition-transform ${
                            cfg().ocr_enabled
                              ? "translate-x-[22px]"
                              : "translate-x-[2px]"
                          }`}
                        />
                      </button>
                    </div>

                    {/* Language pair */}
                    <div class="p-4 bg-white dark:bg-gray-900 rounded-lg border border-gray-200 dark:border-gray-800">
                      <div class="text-sm font-medium mb-3">Language Pair</div>
                      <div class="flex items-center gap-3">
                        <select
                          class="flex-1 px-3 py-2 text-sm bg-gray-50 dark:bg-gray-800 border border-gray-200 dark:border-gray-700 rounded-lg"
                          value={cfg().source_lang}
                          onChange={(e) =>
                            saveConfig({ source_lang: e.currentTarget.value })
                          }
                        >
                          <For each={LANGUAGES}>
                            {(lang) => (
                              <option value={lang.code}>{lang.label}</option>
                            )}
                          </For>
                        </select>
                        <span class="text-gray-400 text-lg">&rarr;</span>
                        <select
                          class="flex-1 px-3 py-2 text-sm bg-gray-50 dark:bg-gray-800 border border-gray-200 dark:border-gray-700 rounded-lg"
                          value={cfg().target_lang}
                          onChange={(e) =>
                            saveConfig({ target_lang: e.currentTarget.value })
                          }
                        >
                          <For each={LANGUAGES.filter((l) => l.code !== "auto")}>
                            {(lang) => (
                              <option value={lang.code}>{lang.label}</option>
                            )}
                          </For>
                        </select>
                      </div>
                    </div>

                    {/* Active provider */}
                    <div class="p-4 bg-white dark:bg-gray-900 rounded-lg border border-gray-200 dark:border-gray-800">
                      <div class="text-sm font-medium mb-3">
                        Active Provider
                      </div>
                      <select
                        class="w-full px-3 py-2 text-sm bg-gray-50 dark:bg-gray-800 border border-gray-200 dark:border-gray-700 rounded-lg"
                        value={cfg().active_provider}
                        onChange={(e) =>
                          saveConfig({
                            active_provider: e.currentTarget.value,
                          })
                        }
                      >
                        <For each={cfg().providers}>
                          {(p) => (
                            <option value={p.name}>
                              {PROVIDER_LABELS[p.name] || p.name}
                            </option>
                          )}
                        </For>
                      </select>
                    </div>

                    {/* Shortcuts */}
                    <div class="p-4 bg-white dark:bg-gray-900 rounded-lg border border-gray-200 dark:border-gray-800">
                      <div class="text-sm font-medium mb-2">Shortcuts</div>
                      <div class="text-xs text-gray-500 space-y-1">
                        <div>
                          <kbd class="px-1.5 py-0.5 bg-gray-100 dark:bg-gray-800 rounded text-[11px] font-mono">
                            Middle Click
                          </kbd>{" "}
                          on selected text to translate (button {cfg().mouse_trigger_button})
                        </div>
                        <div>
                          <kbd class="px-1.5 py-0.5 bg-gray-100 dark:bg-gray-800 rounded text-[11px] font-mono">
                            Cmd+Shift+T
                          </kbd>{" "}
                          fallback shortcut to translate selection
                        </div>
                        <div>
                          <kbd class="px-1.5 py-0.5 bg-gray-100 dark:bg-gray-800 rounded text-[11px] font-mono">
                            Cmd+Shift+O
                          </kbd>{" "}
                          OCR screenshot region translation
                        </div>
                        <div>
                          <kbd class="px-1.5 py-0.5 bg-gray-100 dark:bg-gray-800 rounded text-[11px] font-mono">
                            Esc
                          </kbd>{" "}
                          to close popup
                        </div>
                      </div>
                    </div>

                    {/* Updates */}
                    <div class="p-4 bg-white dark:bg-gray-900 rounded-lg border border-gray-200 dark:border-gray-800">
                      <div class="flex items-center justify-between gap-4">
                        <div>
                          <div class="text-sm font-medium">Updates</div>
                          <Show when={updateStatus().message}>
                            <div
                              class={`mt-1 text-xs ${
                                updateStatus().status === "error"
                                  ? "text-red-500"
                                  : updateStatus().status === "installed"
                                  ? "text-green-500"
                                  : "text-gray-500"
                              }`}
                            >
                              {updateStatus().message}
                            </div>
                          </Show>
                        </div>
                        <button
                          class="text-xs px-3 py-2 bg-gray-100 dark:bg-gray-800 rounded hover:bg-gray-200 dark:hover:bg-gray-700 transition-colors cursor-pointer disabled:opacity-60 disabled:cursor-not-allowed"
                          disabled={
                            updateStatus().status === "checking" ||
                            updateStatus().status === "downloading" ||
                            updateStatus().status === "installed"
                          }
                          onClick={checkForUpdates}
                        >
                          {updateStatus().status === "checking" ||
                          updateStatus().status === "downloading"
                            ? "Working..."
                            : "Check & Install"}
                        </button>
                      </div>
                    </div>
                  </div>
                </Show>

                {/* Providers tab */}
                <Show when={activeTab() === "providers"}>
                  <div class="space-y-4">
                    <For each={cfg().providers}>
                      {(provider) => (
                        <div class="p-4 bg-white dark:bg-gray-900 rounded-lg border border-gray-200 dark:border-gray-800">
                          <div class="flex items-center justify-between mb-3">
                            <div class="text-sm font-medium">
                              {PROVIDER_LABELS[provider.name] || provider.name}
                            </div>
                            <div class="flex items-center gap-2">
                              <Show when={providerStatusLabel(statusFor(provider).status)}>
                                <span
                                  class={`text-xs ${providerStatusTone(
                                    statusFor(provider).status
                                  )}`}
                                >
                                  {providerStatusLabel(statusFor(provider).status)}
                                </span>
                              </Show>
                              <button
                                class="text-xs px-2 py-1 bg-gray-100 dark:bg-gray-800 rounded hover:bg-gray-200 dark:hover:bg-gray-700 transition-colors cursor-pointer disabled:opacity-50 disabled:cursor-not-allowed"
                                disabled={providerBusy(provider)}
                                onClick={() => commitExtraParams(provider)}
                              >
                                Save
                              </button>
                              <button
                                class="text-xs px-2 py-1 bg-gray-100 dark:bg-gray-800 rounded hover:bg-gray-200 dark:hover:bg-gray-700 transition-colors cursor-pointer disabled:opacity-50 disabled:cursor-not-allowed"
                                disabled={providerBusy(provider)}
                                onClick={() => testProvider(provider.name)}
                              >
                                Test
                              </button>
                            </div>
                          </div>

                          <div class="space-y-2">
                            <Show when={provider.name !== "deeplx"}>
                              <input
                                type="password"
                                class="w-full px-3 py-1.5 text-sm bg-gray-50 dark:bg-gray-800 border border-gray-200 dark:border-gray-700 rounded-lg placeholder-gray-400"
                                placeholder="API Key"
                                value={providerDraft(provider).api_key}
                                onInput={(e) =>
                                  updateProviderDraft(
                                    provider,
                                    "api_key",
                                    e.currentTarget.value
                                  )
                                }
                              />
                            </Show>
                            <div class="flex gap-2">
                              <input
                                class="flex-1 px-3 py-1.5 text-sm bg-gray-50 dark:bg-gray-800 border border-gray-200 dark:border-gray-700 rounded-lg placeholder-gray-400"
                                placeholder="Base URL"
                                value={providerDraft(provider).base_url}
                                onInput={(e) =>
                                  updateProviderDraft(
                                    provider,
                                    "base_url",
                                    e.currentTarget.value
                                  )
                                }
                              />
                              <Show when={provider.name !== "deeplx"}>
                                <input
                                  class="w-40 px-3 py-1.5 text-sm bg-gray-50 dark:bg-gray-800 border border-gray-200 dark:border-gray-700 rounded-lg placeholder-gray-400"
                                  placeholder="Model"
                                  value={providerDraft(provider).model}
                                  onInput={(e) =>
                                    updateProviderDraft(
                                      provider,
                                      "model",
                                      e.currentTarget.value
                                    )
                                  }
                                />
                              </Show>
                            </div>
                          </div>

                          {/* Advanced settings toggle for LLM providers */}
                          <Show when={LLM_PROVIDERS.has(provider.name)}>
                            <button
                              class="mt-3 text-xs text-gray-400 hover:text-gray-600 dark:hover:text-gray-300 cursor-pointer flex items-center gap-1"
                              onClick={() =>
                                setExpandedProvider(
                                  expandedProvider() === provider.name
                                    ? null
                                    : provider.name
                                )
                              }
                            >
                              <span
                                class={`inline-block transition-transform ${
                                  expandedProvider() === provider.name
                                    ? "rotate-90"
                                    : ""
                                }`}
                              >
                                &#9654;
                              </span>
                              Advanced Settings
                            </button>

                            <Show when={expandedProvider() === provider.name}>
                              <div class="mt-3 space-y-3 pt-3 border-t border-gray-100 dark:border-gray-800">
                                {/* System Prompt */}
                                <div>
                                  <label class="block text-xs text-gray-500 mb-1">
                                    System Prompt
                                    <span class="ml-1 text-gray-400">
                                      ({"{from}"} {"{to}"} {"{text}"} supported)
                                    </span>
                                  </label>
                                  <textarea
                                    class="w-full px-3 py-2 text-sm bg-gray-50 dark:bg-gray-800 border border-gray-200 dark:border-gray-700 rounded-lg placeholder-gray-400 resize-y min-h-[60px] font-mono"
                                    rows={3}
                                    placeholder="You are a translator..."
                                    value={providerDraft(provider).system_prompt}
                                    onInput={(e) =>
                                      updateProviderDraft(
                                        provider,
                                        "system_prompt",
                                        e.currentTarget.value
                                      )
                                    }
                                  />
                                </div>

                                {/* User Prompt */}
                                <div>
                                  <label class="block text-xs text-gray-500 mb-1">
                                    User Prompt Template
                                    <span class="ml-1 text-gray-400">
                                      ({"{from}"} {"{to}"} {"{text}"} supported)
                                    </span>
                                  </label>
                                  <textarea
                                    class="w-full px-3 py-2 text-sm bg-gray-50 dark:bg-gray-800 border border-gray-200 dark:border-gray-700 rounded-lg placeholder-gray-400 resize-y min-h-[40px] font-mono"
                                    rows={2}
                                    placeholder="{text}"
                                    value={providerDraft(provider).user_prompt}
                                    onInput={(e) =>
                                      updateProviderDraft(
                                        provider,
                                        "user_prompt",
                                        e.currentTarget.value
                                      )
                                    }
                                  />
                                </div>

                                {/* Extra Params */}
                                <div>
                                  <label class="block text-xs text-gray-500 mb-1">
                                    Extra Parameters (JSON)
                                    <span class="ml-1 text-gray-400">
                                      merged into request body
                                    </span>
                                  </label>
                                  <textarea
                                    class="w-full px-3 py-2 text-sm bg-gray-50 dark:bg-gray-800 border border-gray-200 dark:border-gray-700 rounded-lg placeholder-gray-400 resize-y min-h-[40px] font-mono"
                                    rows={2}
                                    placeholder='{"chat_template_kwargs":{"enable_thinking":false}}'
                                    value={extraParamsValue(provider)}
                                    onInput={(e) =>
                                      updateExtraParamsDraft(
                                        provider,
                                        e.currentTarget.value
                                      )
                                    }
                                  />
                                </div>
                              </div>
                            </Show>
                          </Show>

                          <Show when={statusFor(provider).message}>
                            <div
                              class={`mt-2 text-xs p-2 rounded ${
                                statusFor(provider).status === "test_success" ||
                                statusFor(provider).status === "saved"
                                  ? "bg-green-50 dark:bg-green-900/20 text-green-700 dark:text-green-300"
                                  : statusFor(provider).status === "test_error" ||
                                    statusFor(provider).status === "save_error"
                                  ? "bg-red-50 dark:bg-red-900/20 text-red-700 dark:text-red-300"
                                  : statusFor(provider).status === "dirty"
                                  ? "bg-amber-50 dark:bg-amber-900/20 text-amber-700 dark:text-amber-300"
                                  : "bg-gray-50 dark:bg-gray-800 text-gray-500"
                              }`}
                            >
                              {statusFor(provider).message}
                            </div>
                          </Show>
                        </div>
                      )}
                    </For>
                  </div>
                </Show>

                {/* Diagnostics tab */}
                <Show when={activeTab() === "diagnostics"}>
                  <div class="space-y-4">
                    <Show
                      when={diagnostics()}
                      fallback={
                        <div class="p-4 bg-white dark:bg-gray-900 rounded-lg border border-gray-200 dark:border-gray-800 text-sm text-gray-500">
                          Loading diagnostics...
                        </div>
                      }
                    >
                      {(diag) => (
                        <>
                          <div class="p-4 bg-white dark:bg-gray-900 rounded-lg border border-gray-200 dark:border-gray-800">
                            <div class="flex items-start justify-between gap-4 mb-4">
                              <div>
                                <div class="text-sm font-medium">Middle Click Test</div>
                                <div
                                  class={`mt-1 text-xs ${
                                    diag().mouse.accessibility_trusted
                                      ? "text-green-500"
                                      : "text-red-500"
                                  }`}
                                >
                                  Accessibility{" "}
                                  {diag().mouse.accessibility_trusted
                                    ? "Granted"
                                    : "Missing"}
                                </div>
                              </div>
                              <div class="flex gap-2">
                                <button
                                  class="text-xs px-3 py-2 bg-gray-100 dark:bg-gray-800 rounded hover:bg-gray-200 dark:hover:bg-gray-700 transition-colors cursor-pointer"
                                  onClick={openAccessibilitySettings}
                                >
                                  Open Permission
                                </button>
                                <button
                                  class="text-xs px-3 py-2 bg-blue-50 dark:bg-blue-900/30 text-blue-600 dark:text-blue-400 rounded hover:bg-blue-100 dark:hover:bg-blue-900/50 transition-colors cursor-pointer"
                                  onClick={startMiddleClickTest}
                                >
                                  Start Test
                                </button>
                              </div>
                            </div>

                            <div class="grid grid-cols-2 gap-3 text-xs">
                              <div class="p-3 rounded bg-gray-50 dark:bg-gray-800">
                                <div class="text-gray-400 mb-1">Event Tap</div>
                                <div class="font-mono">{diag().mouse.status}</div>
                              </div>
                              <div class="p-3 rounded bg-gray-50 dark:bg-gray-800">
                                <div class="text-gray-400 mb-1">Trigger Button</div>
                                <div class="font-mono">{cfg().mouse_trigger_button}</div>
                              </div>
                              <div class="p-3 rounded bg-gray-50 dark:bg-gray-800">
                                <div class="text-gray-400 mb-1">Last Button</div>
                                <div class="font-mono">
                                  {diag().mouse.last_button ?? "None"}
                                </div>
                              </div>
                              <div class="p-3 rounded bg-gray-50 dark:bg-gray-800">
                                <div class="text-gray-400 mb-1">Last Trigger</div>
                                <div class="font-mono">
                                  {formatMillis(diag().mouse.last_trigger_at)}
                                </div>
                              </div>
                            </div>

                            <Show when={diag().mouse.last_pipeline_result}>
                              <div class="mt-3 text-xs p-3 rounded bg-gray-50 dark:bg-gray-800 text-gray-600 dark:text-gray-300">
                                {diag().mouse.last_pipeline_result}
                              </div>
                            </Show>
                            <Show when={diag().mouse.last_error}>
                              <div class="mt-3 text-xs p-3 rounded bg-red-50 dark:bg-red-900/20 text-red-700 dark:text-red-300">
                                {diag().mouse.last_error}
                              </div>
                            </Show>
                            <Show
                              when={
                                diag().mouse.last_button !== undefined &&
                                diag().mouse.last_button !== null &&
                                diag().mouse.last_button !== cfg().mouse_trigger_button
                              }
                            >
                              <button
                                class="mt-3 text-xs px-3 py-2 bg-amber-50 dark:bg-amber-900/20 text-amber-700 dark:text-amber-300 rounded hover:bg-amber-100 dark:hover:bg-amber-900/40 transition-colors cursor-pointer"
                                onClick={saveLastMouseButton}
                              >
                                Use button {diag().mouse.last_button} as trigger
                              </button>
                            </Show>
                          </div>

                          <div class="p-4 bg-white dark:bg-gray-900 rounded-lg border border-gray-200 dark:border-gray-800">
                            <div class="flex items-start justify-between gap-4 mb-4">
                              <div>
                                <div class="text-sm font-medium">OCR</div>
                                <div
                                  class={`mt-1 text-xs ${
                                    diag().ocr.ready
                                      ? "text-green-500"
                                      : "text-red-500"
                                  }`}
                                >
                                  {diag().ocr.ready
                                    ? "Ready"
                                    : diag().ocr.reason || "Not ready"}
                                </div>
                              </div>
                              <button
                                class="text-xs px-3 py-2 bg-gray-100 dark:bg-gray-800 rounded hover:bg-gray-200 dark:hover:bg-gray-700 transition-colors cursor-pointer"
                                onClick={openScreenRecordingSettings}
                              >
                                Screen Recording
                              </button>
                            </div>

                            <div class="grid grid-cols-2 gap-3 text-xs">
                              <div class="p-3 rounded bg-gray-50 dark:bg-gray-800">
                                <div class="text-gray-400 mb-1">Backend</div>
                                <div class="font-mono">{diag().ocr.backend}</div>
                              </div>
                              <div class="p-3 rounded bg-gray-50 dark:bg-gray-800">
                                <div class="text-gray-400 mb-1">Shortcut</div>
                                <div class="font-mono">
                                  {diag().ocr.enabled ? "enabled" : "disabled"}
                                </div>
                              </div>
                              <div class="p-3 rounded bg-gray-50 dark:bg-gray-800">
                                <div class="text-gray-400 mb-1">Screen Capture</div>
                                <div
                                  class={
                                    diag().ocr.screen_capture_ready
                                      ? "text-green-500"
                                      : "text-red-500"
                                  }
                                >
                                  {diag().ocr.screen_capture_ready
                                    ? "Ready"
                                    : "Not ready"}
                                </div>
                              </div>
                              <div class="p-3 rounded bg-gray-50 dark:bg-gray-800">
                                <div class="text-gray-400 mb-1">Last OCR</div>
                                <div class="font-mono">
                                  {diag().ocr.last_elapsed_ms
                                    ? `${diag().ocr.last_elapsed_ms} ms`
                                    : "None"}
                                </div>
                              </div>
                            </div>

                            <Show when={diag().ocr.last_result}>
                              <div class="mt-3 text-xs p-3 rounded bg-gray-50 dark:bg-gray-800 text-gray-600 dark:text-gray-300">
                                {diag().ocr.last_result}
                              </div>
                            </Show>
                            <Show when={diag().ocr.last_error}>
                              <div class="mt-3 text-xs p-3 rounded bg-red-50 dark:bg-red-900/20 text-red-700 dark:text-red-300">
                                {diag().ocr.last_error}
                              </div>
                            </Show>
                          </div>

                          <div class="p-4 bg-white dark:bg-gray-900 rounded-lg border border-gray-200 dark:border-gray-800">
                            <div class="text-sm font-medium mb-3">Provider Readiness</div>
                            <div
                              class={`text-xs p-3 rounded mb-3 ${
                                diag().active_provider_ready
                                  ? "bg-green-50 dark:bg-green-900/20 text-green-700 dark:text-green-300"
                                  : "bg-red-50 dark:bg-red-900/20 text-red-700 dark:text-red-300"
                              }`}
                            >
                              {diag().active_provider_ready
                                ? "Active provider is ready"
                                : diag().active_provider_reason ||
                                  "Active provider is not ready"}
                            </div>
                            <div class="space-y-2">
                              <For each={diag().providers}>
                                {(provider) => (
                                  <div class="flex items-center justify-between text-xs p-2 rounded bg-gray-50 dark:bg-gray-800">
                                    <span>
                                      {PROVIDER_LABELS[provider.name] ||
                                        provider.name}
                                      {provider.active ? " (active)" : ""}
                                    </span>
                                    <span
                                      class={
                                        provider.ready
                                          ? "text-green-500"
                                          : "text-red-500"
                                      }
                                    >
                                      {provider.ready
                                        ? "Ready"
                                        : provider.reason || "Not ready"}
                                    </span>
                                  </div>
                                )}
                              </For>
                            </div>
                          </div>

                          <div class="p-4 bg-white dark:bg-gray-900 rounded-lg border border-gray-200 dark:border-gray-800">
                            <div class="flex items-center justify-between gap-4 mb-4 pb-4 border-b border-gray-100 dark:border-gray-800">
                              <div>
                                <div class="text-sm font-medium">Debug Logging</div>
                                <div class="text-xs text-gray-500 mt-1">
                                  Extra local diagnostics for clipboard, trigger, and provider failures
                                </div>
                              </div>
                              <button
                                class={`w-11 h-6 rounded-full transition-colors cursor-pointer ${
                                  cfg().debug_logging
                                    ? "bg-blue-500"
                                    : "bg-gray-300 dark:bg-gray-700"
                                }`}
                                onClick={() =>
                                  saveConfig({
                                    debug_logging: !cfg().debug_logging,
                                  })
                                }
                              >
                                <div
                                  class={`w-5 h-5 rounded-full bg-white shadow-sm transition-transform ${
                                    cfg().debug_logging
                                      ? "translate-x-[22px]"
                                      : "translate-x-[2px]"
                                  }`}
                                />
                              </button>
                            </div>
                            <div class="flex items-center justify-between gap-4">
                              <div>
                                <div class="text-sm font-medium">Diagnostics Report</div>
                                <div class="text-xs text-gray-500 mt-1 truncate max-w-[360px]">
                                  {diag().log_dir || "Log directory unavailable"} ·{" "}
                                  {Math.round(diag().log_max_file_size_bytes / 1024)}KB ×{" "}
                                  {diag().log_rotation_keep_files + 1}
                                </div>
                              </div>
                              <div class="flex gap-2">
                                <button
                                  class="text-xs px-3 py-2 bg-gray-100 dark:bg-gray-800 rounded hover:bg-gray-200 dark:hover:bg-gray-700 transition-colors cursor-pointer"
                                  onClick={revealDiagnosticsFolder}
                                >
                                  Reveal
                                </button>
                                <button
                                  class="text-xs px-3 py-2 bg-gray-100 dark:bg-gray-800 rounded hover:bg-gray-200 dark:hover:bg-gray-700 transition-colors cursor-pointer"
                                  onClick={exportDiagnostics}
                                >
                                  Export
                                </button>
                              </div>
                            </div>
                            <Show when={diagnosticsMessage()}>
                              <div class="mt-3 text-xs p-3 rounded bg-gray-50 dark:bg-gray-800 text-gray-600 dark:text-gray-300 break-words">
                                {diagnosticsMessage()}
                              </div>
                            </Show>
                          </div>
                        </>
                      )}
                    </Show>
                  </div>
                </Show>

                {/* History tab */}
                <Show when={activeTab() === "history"}>
                  <div class="space-y-3">
                    <div class="flex items-center justify-between mb-2">
                      <span class="text-sm text-gray-500">
                        {history().length} entries
                      </span>
                      <button
                        class="text-xs px-2 py-1 text-red-500 hover:bg-red-50 dark:hover:bg-red-900/20 rounded transition-colors cursor-pointer"
                        onClick={clearHistory}
                      >
                        Clear All
                      </button>
                    </div>
                    <For
                      each={history()}
                      fallback={
                        <div class="text-center text-sm text-gray-400 py-8">
                          No translation history yet
                        </div>
                      }
                    >
                      {(entry) => (
                        <div class="p-3 bg-white dark:bg-gray-900 rounded-lg border border-gray-200 dark:border-gray-800">
                          <div class="flex items-center justify-between mb-1">
                            <div class="text-[10px] text-gray-400 font-mono">
                              {entry.source_lang} → {entry.target_lang}
                              <span class="ml-1">[{entry.provider}]</span>
                            </div>
                            <div class="text-[10px] text-gray-400">
                              {formatTime(entry.timestamp)}
                            </div>
                          </div>
                          <div class="text-xs text-gray-500 dark:text-gray-400 mb-1 truncate">
                            {entry.original}
                          </div>
                          <div class="text-sm text-gray-900 dark:text-gray-100 truncate">
                            {entry.translated}
                          </div>
                        </div>
                      )}
                    </For>
                  </div>
                </Show>
              </>
            )}
          </Show>
        </div>
      </div>
    </div>
  );
}
