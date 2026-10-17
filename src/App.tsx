import { createSignal, createResource, For, Show, onMount, onCleanup } from "solid-js";
import { listen } from "@tauri-apps/api/event";
import { relaunch } from "@tauri-apps/plugin-process";
import { check, type DownloadEvent } from "@tauri-apps/plugin-updater";
import * as api from "./api/commands";
import type {
  AppConfig,
  ConfigPatch,
  DiagnosticsSnapshot,
  HistoryEntry,
  MouseTriggerState,
  ProviderConfig,
  ProviderUpdate,
} from "./api/types";

/**
 * UI-only state. Everything that crosses the Rust boundary is imported from
 * `./api/types`, which ts-rs generates from the Rust structs — the hand-written
 * copies that used to live here had already drifted from the real payloads.
 */
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
  // Local, and reached over Ollama's OpenAI-compatible endpoint.
  ollama: "Ollama (Local)",
};

const LLM_PROVIDERS = new Set(["openai", "gemini", "claude", "ollama"]);

/** Turn an unknown thrown value into something worth showing in the UI. */
function describeError(error: unknown): string {
  if (error instanceof Error) {
    return error.message;
  }
  return typeof error === "string" ? error : JSON.stringify(error);
}

/**
 * The draft is a full `ProviderConfig`; `ProviderUpdate` is the subset the
 * command accepts. Both are generated, so this stays honest if either changes.
 */
function asProviderUpdate(name: string, draft: ProviderConfig): ProviderUpdate {
  return {
    name,
    baseUrl: draft.baseUrl,
    apiKey: draft.apiKey,
    model: draft.model,
    systemPrompt: draft.systemPrompt,
    userPrompt: draft.userPrompt,
    extraParams: draft.extraParams,
  };
}

export default function App() {
  const [config, { mutate }] = createResource<AppConfig>(() => api.getConfig());
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
  const [diagnostics, setDiagnostics] = createSignal<DiagnosticsSnapshot | null>(null);
  const [diagnosticsMessage, setDiagnosticsMessage] = createSignal("");

  onMount(() => {
    let unlistenConfig: (() => void) | undefined;
    let unlistenMouse: (() => void) | undefined;

    refreshDiagnostics();

    listen("config-changed", async () => {
      try {
        mutate(await api.getConfig());
      } catch (error) {
        console.error("failed to reload config after external change", error);
      }
    }).then((unlisten) => {
      unlistenConfig = unlisten;
    });
    listen<MouseTriggerState>("mouse-trigger-state", (event) => {
      setDiagnostics((prev) =>
        prev
          ? {
              ...prev,
              accessibilityTrusted: event.payload.accessibilityTrusted,
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
      setHistory(await api.getHistory());
    } catch (error) {
      console.error("failed to load history", error);
      setHistory([]);
    }
  }

  async function clearHistory() {
    await api.clearHistory();
    setHistory([]);
  }

  /**
   * Apply a partial change. `ConfigPatch` is generated from Rust, so a key that
   * is not a config field fails to compile here and is refused by Rust if it
   * somehow gets through.
   */
  async function saveConfig(patch: ConfigPatch) {
    mutate(await api.updateConfig(patch));
    await refreshDiagnostics();
  }

  async function refreshDiagnostics() {
    try {
      setDiagnostics(await api.getDiagnosticsSnapshot());
    } catch (error) {
      console.error("failed to read diagnostics", error);
      setDiagnostics(null);
    }
  }

  async function startMiddleClickTest() {
    setDiagnosticsMessage("Press the mouse wheel within 10 seconds.");
    try {
      const mouse = await api.startMiddleClickTest();
      setDiagnostics((prev) =>
        prev
          ? {
              ...prev,
              mouse,
            }
          : prev
      );
    } catch (error) {
      setDiagnosticsMessage(`Failed to start test: ${describeError(error)}`);
    }
  }

  async function saveLastMouseButton() {
    const lastButton = diagnostics()?.mouse.lastButton;
    if (lastButton === undefined || lastButton === null) return;
    try {
      await saveConfig({ mouseTriggerButton: lastButton });
      setDiagnosticsMessage(`Saved button ${lastButton} as the trigger.`);
    } catch (error) {
      setDiagnosticsMessage(`Failed to save trigger button: ${describeError(error)}`);
    }
  }

  async function openAccessibilitySettings() {
    try {
      await api.openAccessibilitySettings();
      setDiagnosticsMessage("Opened macOS Accessibility settings.");
    } catch (error) {
      setDiagnosticsMessage(describeError(error));
    }
  }

  async function exportDiagnostics() {
    try {
      const result = await api.exportDiagnosticsReport();
      setDiagnosticsMessage(`Diagnostics exported: ${result.path}`);
    } catch (error) {
      setDiagnosticsMessage(`Export failed: ${describeError(error)}`);
    }
  }

  async function revealDiagnosticsFolder() {
    try {
      await api.revealDiagnosticsFolder();
    } catch (error) {
      setDiagnosticsMessage(`Reveal failed: ${describeError(error)}`);
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
      a.baseUrl !== b.baseUrl ||
      a.apiKey !== b.apiKey ||
      a.model !== b.model ||
      a.systemPrompt !== b.systemPrompt ||
      a.userPrompt !== b.userPrompt ||
      JSON.stringify(a.extraParams ?? {}) !== JSON.stringify(b.extraParams ?? {})
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
      await api.updateProvider(asProviderUpdate(name, draft));
      mutate(await api.getConfig());
      clearProviderDraft(name);
      setProviderStatus((prev) => ({
        ...prev,
        [name]: { status: "saved", message: "Saved" },
      }));
      await refreshDiagnostics();
      return true;
    } catch (error) {
      setProviderStatus((prev) => ({
        ...prev,
        [name]: { status: "save_error", message: describeError(error) },
      }));
      return false;
    }
  }

  function extraParamsValue(provider: ProviderConfig) {
    return (
      extraParamDrafts()[provider.name] ??
      JSON.stringify(providerDraft(provider).extraParams || {}, null, 2)
    );
  }

  function updateExtraParamsDraft(provider: ProviderConfig, raw: string) {
    setExtraParamDrafts((prev) => ({ ...prev, [provider.name]: raw }));
    setProviderStatus((prev) => ({
      ...prev,
      [provider.name]: { status: "dirty", message: "Unsaved changes" },
    }));
    try {
      updateProviderDraft(provider, "extraParams", JSON.parse(raw));
    } catch {
      // Keep the raw draft so typing invalid intermediate JSON does not get clobbered.
    }
  }

  async function commitExtraParams(provider: ProviderConfig) {
    const raw = extraParamDrafts()[provider.name];
    if (raw === undefined) return commitProvider(provider.name);

    try {
      updateProviderDraft(provider, "extraParams", JSON.parse(raw));
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
      const result = await api.testProvider(name);
      setProviderStatus((prev) => ({
        ...prev,
        [name]: { status: "test_success", message: result },
      }));
    } catch (error) {
      setProviderStatus((prev) => ({
        ...prev,
        [name]: { status: "test_error", message: describeError(error) },
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
        // Single source of truth: the snapshot already carries the version, so
        // there is no second reading that could disagree with what is displayed.
        const version = diagnostics()?.appVersion ?? "";
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
    } catch (error) {
      setUpdateStatus({
        status: "error",
        message: `Update failed: ${describeError(error)}`,
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
            {diagnostics()?.appVersion ? `v${diagnostics()?.appVersion}` : ""}
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
                          Select text or middle-click to translate
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

                    {/* Selection trigger toggle */}
                    <div class="flex items-center justify-between p-4 bg-white dark:bg-gray-900 rounded-lg border border-gray-200 dark:border-gray-800">
                      <div>
                        <div class="text-sm font-medium">Selection Trigger</div>
                        <div class="text-xs text-gray-500">
                          Translate after a deliberate left-button text selection
                        </div>
                      </div>
                      <button
                        class={`w-11 h-6 rounded-full transition-colors cursor-pointer ${
                          cfg().selectionTriggerEnabled
                            ? "bg-blue-500"
                            : "bg-gray-300 dark:bg-gray-700"
                        }`}
                        onClick={() =>
                          saveConfig({
                            selectionTriggerEnabled: !cfg().selectionTriggerEnabled,
                          })
                        }
                      >
                        <div
                          class={`w-5 h-5 rounded-full bg-white shadow-sm transition-transform ${
                            cfg().selectionTriggerEnabled
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
                          value={cfg().sourceLang}
                          onChange={(e) =>
                            saveConfig({ sourceLang: e.currentTarget.value })
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
                          value={cfg().targetLang}
                          onChange={(e) =>
                            saveConfig({ targetLang: e.currentTarget.value })
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
                        value={cfg().activeProvider}
                        onChange={(e) =>
                          saveConfig({
                            activeProvider: e.currentTarget.value,
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
                            Select Text
                          </kbd>{" "}
                          {cfg().selectionTriggerEnabled ? "auto-translates after drag selection" : "disabled"}
                        </div>
                        <div>
                          <kbd class="px-1.5 py-0.5 bg-gray-100 dark:bg-gray-800 rounded text-[11px] font-mono">
                            Middle Click
                          </kbd>{" "}
                          on selected text to translate (button {cfg().mouseTriggerButton})
                        </div>
                        <div>
                          <kbd class="px-1.5 py-0.5 bg-gray-100 dark:bg-gray-800 rounded text-[11px] font-mono">
                            Cmd+Shift+T
                          </kbd>{" "}
                          fallback shortcut to translate selection
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
                                value={providerDraft(provider).apiKey}
                                onInput={(e) =>
                                  updateProviderDraft(
                                    provider,
                                    "apiKey",
                                    e.currentTarget.value
                                  )
                                }
                              />
                            </Show>
                            <div class="flex gap-2">
                              <input
                                class="flex-1 px-3 py-1.5 text-sm bg-gray-50 dark:bg-gray-800 border border-gray-200 dark:border-gray-700 rounded-lg placeholder-gray-400"
                                placeholder="Base URL"
                                value={providerDraft(provider).baseUrl}
                                onInput={(e) =>
                                  updateProviderDraft(
                                    provider,
                                    "baseUrl",
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
                                    value={providerDraft(provider).systemPrompt}
                                    onInput={(e) =>
                                      updateProviderDraft(
                                        provider,
                                        "systemPrompt",
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
                                    value={providerDraft(provider).userPrompt}
                                    onInput={(e) =>
                                      updateProviderDraft(
                                        provider,
                                        "userPrompt",
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
                          <Show when={diag().configLoadProblem}>
                            {(problem) => (
                              <div class="p-4 rounded-lg border border-amber-300 dark:border-amber-800 bg-amber-50 dark:bg-amber-900/30">
                                <div class="text-sm font-medium text-amber-800 dark:text-amber-200">
                                  {problem().kind === "malformed"
                                    ? "Your settings file could not be read"
                                    : "Your settings file is unavailable"}
                                </div>
                                <div class="mt-1 text-xs text-amber-700 dark:text-amber-300 break-all">
                                  {problem().path}
                                </div>
                                <div class="mt-1 text-xs text-amber-700 dark:text-amber-300">
                                  {problem().detail}
                                </div>
                                <Show when={problem().backupPath}>
                                  <div class="mt-2 text-xs text-amber-800 dark:text-amber-200">
                                    The original was kept at{" "}
                                    <span class="font-mono break-all">
                                      {problem().backupPath}
                                    </span>
                                    . Everything below is showing defaults.
                                  </div>
                                </Show>
                              </div>
                            )}
                          </Show>

                          {/* Changes made while reading the settings file. Not an
                              error, but not something to make the user infer
                              either: a provider entry that disappeared or a URL
                              that was rewritten must say so. */}
                          <Show when={diag().configNotices.length > 0}>
                            <div class="p-4 rounded-lg border border-sky-300 dark:border-sky-800 bg-sky-50 dark:bg-sky-900/30">
                              <div class="text-sm font-medium text-sky-800 dark:text-sky-200">
                                Your settings were updated
                              </div>
                              <ul class="mt-2 space-y-1">
                                <For each={diag().configNotices}>
                                  {(notice) => (
                                    <li class="text-xs text-sky-700 dark:text-sky-300">
                                      {notice}
                                    </li>
                                  )}
                                </For>
                              </ul>
                            </div>
                          </Show>

                          <div class="p-4 bg-white dark:bg-gray-900 rounded-lg border border-gray-200 dark:border-gray-800">
                            <div class="flex items-start justify-between gap-4 mb-4">
                              <div>
                                <div class="text-sm font-medium">Middle Click Test</div>
                                <div
                                  class={`mt-1 text-xs ${
                                    diag().mouse.accessibilityTrusted
                                      ? "text-green-500"
                                      : "text-red-500"
                                  }`}
                                >
                                  Accessibility{" "}
                                  {diag().mouse.accessibilityTrusted
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
                                <div class="font-mono">{cfg().mouseTriggerButton}</div>
                              </div>
                              <div class="p-3 rounded bg-gray-50 dark:bg-gray-800">
                                <div class="text-gray-400 mb-1">Selection Trigger</div>
                                <div class="font-mono">
                                  {diag().mouse.selectionTriggerEnabled ? "enabled" : "disabled"}
                                </div>
                              </div>
                              <div class="p-3 rounded bg-gray-50 dark:bg-gray-800">
                                <div class="text-gray-400 mb-1">Last Button</div>
                                <div class="font-mono">
                                  {diag().mouse.lastButton ?? "None"}
                                </div>
                              </div>
                              <div class="p-3 rounded bg-gray-50 dark:bg-gray-800">
                                <div class="text-gray-400 mb-1">Last Trigger</div>
                                <div class="font-mono">
                                  {formatMillis(diag().mouse.lastTriggerAt)}
                                </div>
                              </div>
                            </div>

                            <Show when={diag().mouse.lastPipelineResult}>
                              <div class="mt-3 text-xs p-3 rounded bg-gray-50 dark:bg-gray-800 text-gray-600 dark:text-gray-300">
                                {diag().mouse.lastPipelineResult}
                              </div>
                            </Show>
                            <Show when={diag().mouse.lastError}>
                              <div class="mt-3 text-xs p-3 rounded bg-red-50 dark:bg-red-900/20 text-red-700 dark:text-red-300">
                                {diag().mouse.lastError}
                              </div>
                            </Show>
                            <Show
                              when={
                                diag().mouse.lastButton !== undefined &&
                                diag().mouse.lastButton !== null &&
                                diag().mouse.lastButton !== cfg().mouseTriggerButton
                              }
                            >
                              <button
                                class="mt-3 text-xs px-3 py-2 bg-amber-50 dark:bg-amber-900/20 text-amber-700 dark:text-amber-300 rounded hover:bg-amber-100 dark:hover:bg-amber-900/40 transition-colors cursor-pointer"
                                onClick={saveLastMouseButton}
                              >
                                Use button {diag().mouse.lastButton} as trigger
                              </button>
                            </Show>
                          </div>

                          <div class="p-4 bg-white dark:bg-gray-900 rounded-lg border border-gray-200 dark:border-gray-800">
                            <div class="text-sm font-medium mb-3">Provider Readiness</div>
                            <div
                              class={`text-xs p-3 rounded mb-3 ${
                                diag().activeProviderReady
                                  ? "bg-green-50 dark:bg-green-900/20 text-green-700 dark:text-green-300"
                                  : "bg-red-50 dark:bg-red-900/20 text-red-700 dark:text-red-300"
                              }`}
                            >
                              {diag().activeProviderReady
                                ? "Active provider is ready"
                                : diag().activeProviderReason ||
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
                                  cfg().debugLogging
                                    ? "bg-blue-500"
                                    : "bg-gray-300 dark:bg-gray-700"
                                }`}
                                onClick={() =>
                                  saveConfig({
                                    debugLogging: !cfg().debugLogging,
                                  })
                                }
                              >
                                <div
                                  class={`w-5 h-5 rounded-full bg-white shadow-sm transition-transform ${
                                    cfg().debugLogging
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
                                  {diag().logDir || "Log directory unavailable"} ·{" "}
                                  {Math.round(diag().logMaxFileSizeBytes / 1024)}KB ×{" "}
                                  {diag().logRotationKeepFiles + 1}
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
                              {entry.sourceLang} → {entry.targetLang}
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
