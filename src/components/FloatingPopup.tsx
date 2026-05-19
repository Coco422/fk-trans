import { createSignal, onMount, onCleanup, Show, For } from "solid-js";
import { listen } from "@tauri-apps/api/event";
import { invoke } from "@tauri-apps/api/core";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
import type { TranslationPayload } from "../types/translation";

export default function FloatingPopup() {
  const [data, setData] = createSignal<TranslationPayload | null>(null);
  const [loading, setLoading] = createSignal(false);
  const [error, setError] = createSignal<string | null>(null);
  const [extraResult, setExtraResult] = createSignal<string>("");
  const [copied, setCopied] = createSignal(false);
  const [actionLoading, setActionLoading] = createSignal<string | null>(null);

  let hideTimeout: ReturnType<typeof setTimeout> | undefined;

  function resetHideTimeout() {
    if (hideTimeout) clearTimeout(hideTimeout);
    hideTimeout = setTimeout(() => {
      getCurrentWebviewWindow().hide();
    }, 30_000);
  }

  onMount(async () => {
    const win = getCurrentWebviewWindow();

    const unlisten = await listen<TranslationPayload>(
      "translation-ready",
      (event) => {
        setData(event.payload);
        setExtraResult("");
        setError(null);
        setLoading(false);
        resetHideTimeout();
      }
    );

    const unlistenLoading = await listen("translation-started", () => {
      setLoading(true);
      setError(null);
    });

    const unlistenError = await listen<string>("translation-error", (event) => {
      setError(event.payload);
      setLoading(false);
      setData(null);
    });

    const unlistenBlur = await win.onCloseRequested(() => {
      // prevent close, just hide
    });

    document.addEventListener("keydown", handleKeydown);

    // Hide on blur (click outside)
    const blurHandler = () => {
      // Small delay to allow button clicks to register
      setTimeout(() => {
        if (!document.hasFocus()) {
          win.hide();
        }
      }, 150);
    };
    window.addEventListener("blur", blurHandler);

    onCleanup(() => {
      unlisten();
      unlistenLoading();
      unlistenError();
      unlistenBlur();
      document.removeEventListener("keydown", handleKeydown);
      window.removeEventListener("blur", blurHandler);
      if (hideTimeout) clearTimeout(hideTimeout);
    });
  });

  function handleKeydown(e: KeyboardEvent) {
    if (e.key === "Escape") {
      getCurrentWebviewWindow().hide();
    }
  }

  async function handleAction(action: string) {
    if (!data()) return;
    setActionLoading(action);
    setExtraResult("");
    try {
      const result = await invoke<string>("ai_action", {
        text: data()!.original,
        action,
        sourceLang: data()!.result.source_lang,
        targetLang: data()!.result.target_lang,
      });
      setExtraResult(result);
      resetHideTimeout();
    } catch (e) {
      setExtraResult(`Error: ${e}`);
    } finally {
      setActionLoading(null);
    }
  }

  async function handleCopy(text: string) {
    try {
      await navigator.clipboard.writeText(text);
      setCopied(true);
      setTimeout(() => setCopied(false), 1500);
    } catch {
      // fallback: invoke tauri clipboard
      await invoke("plugin:clipboard-manager|write_text", { text });
      setCopied(true);
      setTimeout(() => setCopied(false), 1500);
    }
  }

  return (
    <div class="w-full h-full flex items-start justify-start p-0 m-0">
      {/* Error state */}
      <Show when={error()}>
        <div class="bg-white/95 dark:bg-gray-900/95 backdrop-blur-md rounded-xl shadow-2xl border border-red-200 dark:border-red-900 p-4 w-[380px] animate-fade-in">
          <div class="flex items-center gap-2 text-red-500 mb-2">
            <svg class="w-4 h-4 shrink-0" fill="none" viewBox="0 0 24 24" stroke="currentColor">
              <path stroke-linecap="round" stroke-linejoin="round" stroke-width={2} d="M12 9v2m0 4h.01M21 12a9 9 0 11-18 0 9 9 0 0118 0z" />
            </svg>
            <span class="text-sm font-medium">Translation Error</span>
          </div>
          <p class="text-xs text-gray-600 dark:text-gray-400">{error()}</p>
          <button
            class="mt-3 text-xs px-3 py-1.5 rounded-lg bg-red-50 dark:bg-red-900/30 text-red-600 dark:text-red-400 hover:bg-red-100 dark:hover:bg-red-900/50 transition-colors cursor-pointer"
            onClick={() => getCurrentWebviewWindow().hide()}
          >
            Dismiss
          </button>
        </div>
      </Show>

      {/* Loading state */}
      <Show when={loading() && !data() && !error()}>
        <div class="bg-white/95 dark:bg-gray-900/95 backdrop-blur-md rounded-xl shadow-2xl border border-gray-200 dark:border-gray-700 p-4 w-[380px] animate-fade-in">
          <div class="flex items-center gap-3 mb-3">
            <div class="relative w-5 h-5">
              <div class="absolute inset-0 rounded-full border-2 border-blue-200 dark:border-blue-800" />
              <div class="absolute inset-0 rounded-full border-2 border-transparent border-t-blue-500 animate-spin" />
            </div>
            <span class="text-sm font-medium text-gray-700 dark:text-gray-300">
              Translating...
            </span>
          </div>
          {/* Shimmer placeholder */}
          <div class="space-y-2">
            <div class="h-3 w-3/4 rounded bg-gradient-to-r from-gray-200 via-gray-100 to-gray-200 dark:from-gray-700 dark:via-gray-600 dark:to-gray-700 animate-pulse" />
            <div class="h-3 w-1/2 rounded bg-gradient-to-r from-gray-200 via-gray-100 to-gray-200 dark:from-gray-700 dark:via-gray-600 dark:to-gray-700 animate-pulse" />
          </div>
        </div>
      </Show>

      {/* Translation result */}
      <Show when={data()}>
        {(payload) => (
          <div class="bg-white/95 dark:bg-gray-900/95 backdrop-blur-md rounded-xl shadow-2xl border border-gray-200 dark:border-gray-700 p-4 w-[380px] animate-result-in">
            {/* Language pair + provider */}
            <div class="flex items-center justify-between mb-2">
              <div class="text-xs text-gray-500 dark:text-gray-400 font-mono">
                {payload().result.source_lang} → {payload().result.target_lang}
              </div>
              <div class="flex items-center gap-2">
                <span class="text-[10px] px-1.5 py-0.5 rounded bg-gray-100 dark:bg-gray-800 text-gray-400 dark:text-gray-500">
                  {payload().result.provider}
                </span>
                {loading() && (
                  <div class="w-3 h-3 border border-gray-300 border-t-blue-500 rounded-full animate-spin" />
                )}
              </div>
            </div>

            {/* Original text */}
            <div class="text-sm text-gray-500 dark:text-gray-400 mb-3 pb-3 border-b border-gray-200 dark:border-gray-700 max-h-20 overflow-y-auto leading-relaxed">
              {payload().original}
            </div>

            {/* Translation */}
            <div class="text-base text-gray-900 dark:text-white mb-2 whitespace-pre-wrap max-h-48 overflow-y-auto leading-relaxed">
              {payload().result.translated}
            </div>

            {/* Extra result (explain/dict/summary) */}
            <Show when={extraResult()}>
              <div class="text-sm text-gray-600 dark:text-gray-300 mt-2 pt-2 border-t border-gray-200 dark:border-gray-700 whitespace-pre-wrap max-h-64 overflow-y-auto leading-relaxed">
                {extraResult()}
              </div>
            </Show>

            {/* Action buttons */}
            <div class="flex gap-1.5 mt-3 pt-3 border-t border-gray-200 dark:border-gray-700">
              <For each={["explain", "dict", "summary"] as const}>
                {(action) => (
                  <button
                    class="text-xs px-2.5 py-1 rounded-lg bg-gray-100 dark:bg-gray-800 hover:bg-gray-200 dark:hover:bg-gray-700 transition-colors capitalize cursor-pointer disabled:opacity-50 disabled:cursor-not-allowed"
                    disabled={actionLoading() !== null}
                    onClick={() => handleAction(action)}
                  >
                    {actionLoading() === action ? (
                      <span class="inline-flex items-center gap-1">
                        <span class="w-3 h-3 border border-gray-400 border-t-transparent rounded-full animate-spin inline-block" />
                        {action}
                      </span>
                    ) : (
                      action
                    )}
                  </button>
                )}
              </For>
              <button
                class="text-xs px-2.5 py-1 rounded-lg bg-blue-50 dark:bg-blue-900/30 text-blue-600 dark:text-blue-400 hover:bg-blue-100 dark:hover:bg-blue-900/50 transition-colors ml-auto cursor-pointer"
                onClick={() => handleCopy(payload().result.translated)}
              >
                {copied() ? "Copied!" : "Copy"}
              </button>
            </div>
          </div>
        )}
      </Show>
    </div>
  );
}
