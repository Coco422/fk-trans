import { createSignal, onMount, onCleanup, Show, For } from "solid-js";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import {
  aiAction,
  writeClipboard,
  type TranslationReadyPayload,
} from "../api/commands";

/** Extra things the user can ask for about the selected text. */
const ACTIONS = [
  { id: "explain", label: "Explain" },
  { id: "summary", label: "Summary" },
  { id: "polish", label: "Polish" },
  { id: "dict", label: "Dict" },
] as const;

const AUTO_HIDE_MS = 30_000;

export default function FloatingPopup() {
  const [data, setData] = createSignal<TranslationReadyPayload | null>(null);
  const [loading, setLoading] = createSignal(false);
  const [error, setError] = createSignal<string | null>(null);
  const [extraResult, setExtraResult] = createSignal<string>("");
  const [copied, setCopied] = createSignal(false);
  const [actionLoading, setActionLoading] = createSignal<string | null>(null);

  let hideTimeout: ReturnType<typeof setTimeout> | undefined;
  let copiedTimeout: ReturnType<typeof setTimeout> | undefined;

  function clearHideTimeout() {
    if (hideTimeout) {
      clearTimeout(hideTimeout);
    }
    hideTimeout = undefined;
  }

  /**
   * Restart the idle timer. Called on every state change, not just on success:
   * a slow or failed translation used to leave the original timer running, so
   * the popup could hide while it was still loading.
   */
  function keepAlive() {
    clearHideTimeout();
    hideTimeout = setTimeout(hidePopup, AUTO_HIDE_MS);
  }

  function hidePopup() {
    clearHideTimeout();
    void getCurrentWindow()
      .hide()
      .catch((reason) => console.error("failed to hide popup", reason));
  }

  onMount(() => {
    const win = getCurrentWindow();
    const unlisten: Array<() => void> = [];

    void (async () => {
      unlisten.push(
        await listen<TranslationReadyPayload>("translation-ready", (event) => {
          setData(event.payload);
          setExtraResult("");
          setError(null);
          setLoading(false);
          keepAlive();
        })
      );

      unlisten.push(
        await listen("translation-started", () => {
          setLoading(true);
          setData(null);
          setError(null);
          setExtraResult("");
          keepAlive();
        })
      );

      unlisten.push(
        await listen<string>("translation-error", (event) => {
          setError(event.payload);
          setLoading(false);
          setData(null);
          keepAlive();
        })
      );

      // The window is hidden rather than closed, so a close request is refused.
      unlisten.push(
        await win.onCloseRequested((event) => {
          event.preventDefault();
          hidePopup();
        })
      );
    })();

    document.addEventListener("keydown", handleKeydown);

    onCleanup(() => {
      for (const stop of unlisten) {
        stop();
      }
      document.removeEventListener("keydown", handleKeydown);
      clearHideTimeout();
      if (copiedTimeout) {
        clearTimeout(copiedTimeout);
      }
    });
  });

  function handleKeydown(event: KeyboardEvent) {
    if (event.key === "Escape") {
      hidePopup();
    }
  }

  function startDrag(event: MouseEvent) {
    if (event.button !== 0) {
      return;
    }
    void getCurrentWindow()
      .startDragging()
      .catch(() => {});
  }

  async function handleAction(action: string) {
    const payload = data();
    if (!payload) {
      return;
    }

    setActionLoading(action);
    setExtraResult("");
    keepAlive();
    try {
      const result = await aiAction({
        text: payload.original,
        action,
        sourceLang: payload.result.sourceLang,
        targetLang: payload.result.targetLang,
      });
      setExtraResult(result);
      keepAlive();
    } catch (reason) {
      setExtraResult(reason instanceof Error ? reason.message : String(reason));
    } finally {
      setActionLoading(null);
    }
  }

  async function handleCopy(text: string) {
    await writeClipboard(text).catch((reason) => {
      console.error("failed to copy translation", reason);
    });
    setCopied(true);
    if (copiedTimeout) {
      clearTimeout(copiedTimeout);
    }
    copiedTimeout = setTimeout(() => setCopied(false), 1500);
  }

  const CloseButton = (props: { onMouseDown: (event: MouseEvent) => void }) => (
    <button
      class="w-6 h-6 grid place-items-center rounded-md text-gray-400 hover:text-gray-700 hover:bg-gray-100 dark:hover:text-gray-200 dark:hover:bg-gray-800 cursor-pointer"
      aria-label="Close"
      onMouseDown={props.onMouseDown}
      onClick={hidePopup}
    >
      ×
    </button>
  );

  return (
    <div class="w-full h-full flex items-start justify-start p-3 m-0 overflow-hidden">
      <Show when={error()}>
        <div class="bg-white/95 dark:bg-gray-900/95 backdrop-blur-md rounded-xl shadow-sm p-4 w-[376px] animate-fade-in">
          <div
            class="flex items-center gap-2 text-red-500 mb-2 cursor-move select-none"
            onMouseDown={startDrag}
          >
            <svg
              class="w-4 h-4 shrink-0"
              fill="none"
              viewBox="0 0 24 24"
              stroke="currentColor"
            >
              <path
                stroke-linecap="round"
                stroke-linejoin="round"
                stroke-width={2}
                d="M12 9v2m0 4h.01M21 12a9 9 0 11-18 0 9 9 0 0118 0z"
              />
            </svg>
            <span class="text-sm font-medium">Translation Error</span>
            <div class="ml-auto">
              <CloseButton onMouseDown={(event) => event.stopPropagation()} />
            </div>
          </div>
          <p class="text-xs text-gray-600 dark:text-gray-400">{error()}</p>
          <button
            class="mt-3 text-xs px-3 py-1.5 rounded-lg bg-red-50 dark:bg-red-900/30 text-red-600 dark:text-red-400 hover:bg-red-100 dark:hover:bg-red-900/50 transition-colors cursor-pointer"
            onClick={hidePopup}
          >
            Dismiss
          </button>
        </div>
      </Show>

      <Show when={loading() && !data() && !error()}>
        <div class="bg-white/95 dark:bg-gray-900/95 backdrop-blur-md rounded-xl shadow-sm p-4 w-[376px] animate-fade-in">
          <div
            class="flex items-center gap-3 mb-3 cursor-move select-none"
            onMouseDown={startDrag}
          >
            <div class="relative w-5 h-5">
              <div class="absolute inset-0 rounded-full border-2 border-blue-200 dark:border-blue-800" />
              <div class="absolute inset-0 rounded-full border-2 border-transparent border-t-blue-500 animate-spin" />
            </div>
            <span class="text-sm font-medium text-gray-700 dark:text-gray-300">
              Translating...
            </span>
            <div class="ml-auto">
              <CloseButton onMouseDown={(event) => event.stopPropagation()} />
            </div>
          </div>
          <div class="space-y-2">
            <div class="h-3 w-3/4 rounded bg-gradient-to-r from-gray-200 via-gray-100 to-gray-200 dark:from-gray-700 dark:via-gray-600 dark:to-gray-700 animate-pulse" />
            <div class="h-3 w-1/2 rounded bg-gradient-to-r from-gray-200 via-gray-100 to-gray-200 dark:from-gray-700 dark:via-gray-600 dark:to-gray-700 animate-pulse" />
          </div>
        </div>
      </Show>

      <Show when={data()}>
        {(payload) => (
          <div class="bg-white/95 dark:bg-gray-900/95 backdrop-blur-md rounded-xl shadow-sm p-4 w-[376px] animate-result-in">
            <div
              class="flex items-center justify-between mb-2 cursor-move select-none"
              onMouseDown={startDrag}
            >
              <div class="text-xs text-gray-500 dark:text-gray-400 font-mono">
                {payload().result.sourceLang} → {payload().result.targetLang}
              </div>
              <div class="flex items-center gap-2">
                <span class="text-[10px] px-1.5 py-0.5 rounded bg-gray-100 dark:bg-gray-800 text-gray-400 dark:text-gray-500">
                  {payload().result.provider}
                </span>
                <Show when={loading()}>
                  <div class="w-3 h-3 border border-gray-300 border-t-blue-500 rounded-full animate-spin" />
                </Show>
                <CloseButton onMouseDown={(event) => event.stopPropagation()} />
              </div>
            </div>

            <div class="popup-scroll text-sm text-gray-500 dark:text-gray-400 mb-3 pb-3 border-b border-gray-200 dark:border-gray-700 max-h-20 overflow-y-auto leading-relaxed">
              {payload().original}
            </div>

            <div class="popup-scroll text-base text-gray-900 dark:text-white mb-2 whitespace-pre-wrap max-h-48 overflow-y-auto leading-relaxed">
              {payload().result.translated}
            </div>

            <Show when={extraResult()}>
              <div class="popup-scroll text-sm text-gray-600 dark:text-gray-300 mt-2 pt-2 border-t border-gray-200 dark:border-gray-700 whitespace-pre-wrap max-h-64 overflow-y-auto leading-relaxed">
                {extraResult()}
              </div>
            </Show>

            <div class="flex gap-1.5 mt-3 pt-3 border-t border-gray-200 dark:border-gray-700">
              <For each={ACTIONS}>
                {(action) => (
                  <button
                    class="text-xs px-2.5 py-1 rounded-lg bg-gray-100 dark:bg-gray-800 hover:bg-gray-200 dark:hover:bg-gray-700 transition-colors capitalize cursor-pointer disabled:opacity-50 disabled:cursor-not-allowed"
                    disabled={actionLoading() !== null}
                    onClick={() => handleAction(action.id)}
                  >
                    {actionLoading() === action.id ? (
                      <span class="inline-flex items-center gap-1">
                        <span class="w-3 h-3 border border-gray-400 border-t-transparent rounded-full animate-spin inline-block" />
                        {action.label}
                      </span>
                    ) : (
                      action.label
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
