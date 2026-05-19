import { createSignal, createResource, For, Show, onMount, onCleanup } from "solid-js";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

interface ProviderConfig {
  name: string;
  base_url: string;
  api_key: string;
  model: string;
}

interface AppConfig {
  enabled: boolean;
  source_lang: string;
  target_lang: string;
  active_provider: string;
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

export default function App() {
  const [config, { mutate }] = createResource<AppConfig>(() =>
    invoke("get_config")
  );
  const [history, setHistory] = createSignal<HistoryEntry[]>([]);
  const [testResult, setTestResult] = createSignal<
    Record<string, { status: string; message: string }>
  >({});
  const [activeTab, setActiveTab] = createSignal<
    "general" | "providers" | "history"
  >("general");

  onMount(async () => {
    const unlisten = await listen("config-changed", async () => {
      try {
        const updated = await invoke<AppConfig>("get_config");
        mutate(updated);
      } catch {}
    });
    onCleanup(unlisten);
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
  }

  async function saveProvider(
    name: string,
    field: keyof ProviderConfig,
    value: string
  ) {
    const cfg = config();
    if (!cfg) return;
    const provider = cfg.providers.find((p) => p.name === name);
    if (!provider) return;
    await invoke("update_provider", {
      name,
      base_url: field === "base_url" ? value : provider.base_url,
      api_key: field === "api_key" ? value : provider.api_key,
      model: field === "model" ? value : provider.model,
    });
    const updated = await invoke<AppConfig>("get_config");
    mutate(updated);
  }

  async function testProvider(name: string) {
    setTestResult((prev) => ({
      ...prev,
      [name]: { status: "loading", message: "Testing..." },
    }));
    try {
      const result = await invoke<string>("test_provider", {
        providerName: name,
      });
      setTestResult((prev) => ({
        ...prev,
        [name]: { status: "success", message: result },
      }));
    } catch (e) {
      setTestResult((prev) => ({
        ...prev,
        [name]: { status: "error", message: String(e) },
      }));
    }
  }

  function formatTime(ts: number) {
    return new Date(ts * 1000).toLocaleString();
  }

  return (
    <div class="min-h-screen bg-gray-50 dark:bg-gray-950 text-gray-900 dark:text-gray-100">
      <div class="max-w-2xl mx-auto p-6">
        <div class="flex items-center justify-between mb-6">
          <h1 class="text-xl font-semibold">fk-trans</h1>
          <span class="text-xs text-gray-400">v0.1.0</span>
        </div>

        {/* Tab bar */}
        <div class="flex gap-1 mb-6 border-b border-gray-200 dark:border-gray-800">
          <For each={["general", "providers", "history"] as const}>
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
                  : "History"}
              </button>
            )}
          </For>
        </div>

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
                        on selected text to translate
                      </div>
                      <div>
                        <kbd class="px-1.5 py-0.5 bg-gray-100 dark:bg-gray-800 rounded text-[11px] font-mono">
                          Cmd+Shift+T
                        </kbd>{" "}
                        global shortcut to translate selection
                      </div>
                      <div>
                        <kbd class="px-1.5 py-0.5 bg-gray-100 dark:bg-gray-800 rounded text-[11px] font-mono">
                          Esc
                        </kbd>{" "}
                        to close popup
                      </div>
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
                            <Show when={testResult()[provider.name]}>
                              <span
                                class={`text-xs ${
                                  testResult()[provider.name].status ===
                                  "success"
                                    ? "text-green-500"
                                    : testResult()[provider.name].status ===
                                      "error"
                                    ? "text-red-500"
                                    : "text-gray-400"
                                }`}
                              >
                                {testResult()[provider.name].status ===
                                "loading"
                                  ? "Testing..."
                                  : testResult()[provider.name].status ===
                                    "success"
                                  ? "OK"
                                  : "Failed"}
                              </span>
                            </Show>
                            <button
                              class="text-xs px-2 py-1 bg-gray-100 dark:bg-gray-800 rounded hover:bg-gray-200 dark:hover:bg-gray-700 transition-colors cursor-pointer"
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
                              value={provider.api_key}
                              onChange={(e) =>
                                saveProvider(
                                  provider.name,
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
                              value={provider.base_url}
                              onChange={(e) =>
                                saveProvider(
                                  provider.name,
                                  "base_url",
                                  e.currentTarget.value
                                )
                              }
                            />
                            <Show when={provider.name !== "deeplx"}>
                              <input
                                class="w-40 px-3 py-1.5 text-sm bg-gray-50 dark:bg-gray-800 border border-gray-200 dark:border-gray-700 rounded-lg placeholder-gray-400"
                                placeholder="Model"
                                value={provider.model}
                                onChange={(e) =>
                                  saveProvider(
                                    provider.name,
                                    "model",
                                    e.currentTarget.value
                                  )
                                }
                              />
                            </Show>
                          </div>
                        </div>

                        <Show when={testResult()[provider.name]?.message}>
                          <div
                            class={`mt-2 text-xs p-2 rounded ${
                              testResult()[provider.name].status === "success"
                                ? "bg-green-50 dark:bg-green-900/20 text-green-700 dark:text-green-300"
                                : testResult()[provider.name].status === "error"
                                ? "bg-red-50 dark:bg-red-900/20 text-red-700 dark:text-red-300"
                                : "bg-gray-50 dark:bg-gray-800 text-gray-500"
                            }`}
                          >
                            {testResult()[provider.name].message}
                          </div>
                        </Show>
                      </div>
                    )}
                  </For>
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
  );
}
