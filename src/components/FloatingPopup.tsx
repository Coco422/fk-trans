import { createSignal, onMount, onCleanup, Show, For } from "solid-js";
import { listen } from "@tauri-apps/api/event";
import TriggerDot from "./TriggerDot";
import {
  aiAction,
  dismissTriggerPanel,
  expandTriggerPanel,
  resizeTriggerPanel,
  writeClipboard,
  type TranslationReadyPayload,
} from "../api/commands";

/** Extra things the user can ask for about the selected text. */
const ACTIONS = [
  { id: "explain", label: "解释" },
  { id: "summary", label: "总结" },
  { id: "polish", label: "润色" },
  { id: "dict", label: "词典" },
] as const;

const AUTO_HIDE_MS = 30_000;

/**
 * Which of the two things this window is showing.
 *
 * They share a window because they are the same thought: the point is what a
 * selection looks like before the user has asked for anything, and the card is
 * what it becomes afterwards. A second window would mean a second thing to keep
 * positioned, shown and hidden in step.
 */
type Mode = "point" | "card";

export default function FloatingPopup() {
  const [mode, setMode] = createSignal<Mode>("point");
  const [data, setData] = createSignal<TranslationReadyPayload | null>(null);
  const [loading, setLoading] = createSignal(false);
  const [error, setError] = createSignal<string | null>(null);
  const [extraResult, setExtraResult] = createSignal<string>("");
  const [copied, setCopied] = createSignal(false);
  const [actionLoading, setActionLoading] = createSignal<string | null>(null);

  let content: HTMLDivElement | undefined;
  let hideTimeout: ReturnType<typeof setTimeout> | undefined;
  let copiedTimeout: ReturnType<typeof setTimeout> | undefined;
  let lastReportedHeight = 0;

  function clearHideTimeout() {
    if (hideTimeout) {
      clearTimeout(hideTimeout);
    }
    hideTimeout = undefined;
  }

  /**
   * Restart the idle timer. Called on every state change, not just on success:
   * a slow or failed translation used to leave the original timer running, so
   * the card could hide while it was still loading.
   */
  function keepAlive() {
    clearHideTimeout();
    hideTimeout = setTimeout(dismiss, AUTO_HIDE_MS);
  }

  /**
   * Hide the panel through Rust, which owns its state.
   *
   * Calling `getCurrentWindow().hide()` here instead would leave Rust believing
   * the panel was still on screen, and the next transition would be applied to a
   * window that had already gone.
   */
  function dismiss() {
    clearHideTimeout();
    void dismissTriggerPanel().catch((reason) => {
      console.error("failed to dismiss the panel", reason);
    });
  }

  /**
   * Tell Rust how tall the content turned out.
   *
   * The observer watches the content wrapper, which is sized by what is in it in
   * both modes — so in point mode this reports the dot and nothing happens,
   * because Rust only resizes an open card.
   */
  function reportHeight() {
    const node = content;
    if (!node) {
      return;
    }
    const height = Math.ceil(node.getBoundingClientRect().height);
    // Rust ignores anything under a pixel too, but a sub-pixel wobble here would
    // be a command and a window resize on every frame.
    if (Math.abs(height - lastReportedHeight) < 1) {
      return;
    }
    lastReportedHeight = height;
    void resizeTriggerPanel(height).catch(() => {});
  }

  onMount(() => {
    const unlisten: Array<() => void> = [];

    const observer = new ResizeObserver(reportHeight);
    if (content) {
      observer.observe(content);
    }

    void (async () => {
      unlisten.push(
        await listen("trigger-point", () => {
          // A new selection supersedes whatever was on screen, including a card
          // the user was still reading: they moved on and selected something
          // else, so the old answer is no longer the answer to anything.
          setMode("point");
          setData(null);
          setError(null);
          setLoading(false);
          setExtraResult("");
          clearHideTimeout();
          lastReportedHeight = 0;
          reportHeight();
        })
      );

      unlisten.push(
        await listen<TranslationReadyPayload>("translation-ready", (event) => {
          setMode("card");
          setData(event.payload);
          setExtraResult("");
          setError(null);
          setLoading(false);
          keepAlive();
        })
      );

      unlisten.push(
        await listen("translation-started", () => {
          setMode("card");
          setLoading(true);
          setData(null);
          setError(null);
          setExtraResult("");
          keepAlive();
        })
      );

      unlisten.push(
        await listen<string>("translation-error", (event) => {
          setMode("card");
          setError(event.payload);
          setLoading(false);
          setData(null);
          keepAlive();
        })
      );
    })();

    document.addEventListener("keydown", handleKeydown);

    onCleanup(() => {
      for (const stop of unlisten) {
        stop();
      }
      observer.disconnect();
      document.removeEventListener("keydown", handleKeydown);
      clearHideTimeout();
      if (copiedTimeout) {
        clearTimeout(copiedTimeout);
      }
    });
  });

  function handleKeydown(event: KeyboardEvent) {
    if (event.key === "Escape") {
      dismiss();
    }
  }

  async function activate() {
    lastReportedHeight = 0;
    try {
      await expandTriggerPanel();
    } catch (reason) {
      // Losing the race with the expiry timer is ordinary, not an error worth
      // showing: by the time the click landed, the point had already gone.
      console.warn("the trigger point was already gone", reason);
    }
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

  return (
    // No padding: the window is sized to this content exactly, and the placement
    // maths in Rust assumes the window *is* the box.
    <div ref={content} class="w-full">
      <Show when={mode() === "point"}>
        <TriggerDot onActivate={activate} />
      </Show>

      <Show when={mode() === "card"}>
        <Show when={error()}>
          <div class="w-[380px] rounded-xl border border-red-500/20 bg-white/95 p-4 shadow-sm backdrop-blur-md dark:bg-gray-900/95">
            <div class="mb-2 flex items-center gap-2 text-red-500">
              <svg class="h-4 w-4 shrink-0" fill="none" viewBox="0 0 24 24" stroke="currentColor">
                <path
                  stroke-linecap="round"
                  stroke-linejoin="round"
                  stroke-width={2}
                  d="M12 9v2m0 4h.01M21 12a9 9 0 11-18 0 9 9 0 0118 0z"
                />
              </svg>
              <span class="text-sm font-medium">翻译失败</span>
              <div class="ml-auto">
                <CloseButton onClick={dismiss} />
              </div>
            </div>
            <p class="text-xs text-gray-600 dark:text-gray-400">{error()}</p>
          </div>
        </Show>

        <Show when={loading() && !data() && !error()}>
          <div class="w-[380px] rounded-xl bg-white/95 p-4 shadow-sm backdrop-blur-md dark:bg-gray-900/95">
            <div class="mb-3 flex items-center gap-3">
              <div class="relative h-5 w-5">
                <div class="absolute inset-0 rounded-full border-2 border-blue-200 dark:border-blue-800" />
                <div class="absolute inset-0 animate-spin rounded-full border-2 border-transparent border-t-blue-500" />
              </div>
              <span class="text-sm font-medium text-gray-700 dark:text-gray-300">翻译中…</span>
              <div class="ml-auto">
                <CloseButton onClick={dismiss} />
              </div>
            </div>
            <div class="space-y-2">
              <div class="h-3 w-3/4 animate-pulse rounded bg-gradient-to-r from-gray-200 via-gray-100 to-gray-200 dark:from-gray-700 dark:via-gray-600 dark:to-gray-700" />
              <div class="h-3 w-1/2 animate-pulse rounded bg-gradient-to-r from-gray-200 via-gray-100 to-gray-200 dark:from-gray-700 dark:via-gray-600 dark:to-gray-700" />
            </div>
          </div>
        </Show>

        <Show when={data()}>
          {(payload) => (
            <div class="w-[380px] animate-result-in rounded-xl bg-white/95 p-4 shadow-sm backdrop-blur-md dark:bg-gray-900/95">
              <div class="mb-2 flex items-center justify-between">
                <div class="font-mono text-xs text-gray-500 dark:text-gray-400">
                  {payload().result.sourceLang} → {payload().result.targetLang}
                </div>
                <div class="flex items-center gap-2">
                  <span class="rounded bg-gray-100 px-1.5 py-0.5 text-[10px] text-gray-400 dark:bg-gray-800 dark:text-gray-500">
                    {payload().result.provider}
                  </span>
                  <Show when={loading()}>
                    <div class="h-3 w-3 animate-spin rounded-full border border-gray-300 border-t-blue-500" />
                  </Show>
                  <CloseButton onClick={dismiss} />
                </div>
              </div>

              <div class="popup-scroll mb-3 max-h-20 overflow-y-auto border-b border-gray-200 pb-3 text-sm leading-relaxed text-gray-500 dark:border-gray-700 dark:text-gray-400">
                {payload().original}
              </div>

              <div class="popup-scroll mb-2 max-h-48 overflow-y-auto whitespace-pre-wrap text-base leading-relaxed text-gray-900 dark:text-white">
                {payload().result.translated}
              </div>

              <Show when={extraResult()}>
                <div class="popup-scroll mt-2 max-h-64 overflow-y-auto whitespace-pre-wrap border-t border-gray-200 pt-2 text-sm leading-relaxed text-gray-600 dark:border-gray-700 dark:text-gray-300">
                  {extraResult()}
                </div>
              </Show>

              <div class="mt-3 flex gap-1.5 border-t border-gray-200 pt-3 dark:border-gray-700">
                <For each={ACTIONS}>
                  {(action) => (
                    <button
                      class="cursor-pointer rounded-lg bg-gray-100 px-2.5 py-1 text-xs transition-colors hover:bg-gray-200 disabled:cursor-not-allowed disabled:opacity-50 dark:bg-gray-800 dark:hover:bg-gray-700"
                      disabled={actionLoading() !== null}
                      onClick={() => handleAction(action.id)}
                    >
                      {actionLoading() === action.id ? (
                        <span class="inline-flex items-center gap-1">
                          <span class="inline-block h-3 w-3 animate-spin rounded-full border border-gray-400 border-t-transparent" />
                          {action.label}
                        </span>
                      ) : (
                        action.label
                      )}
                    </button>
                  )}
                </For>
                <button
                  class="ml-auto cursor-pointer rounded-lg bg-blue-50 px-2.5 py-1 text-xs text-blue-600 transition-colors hover:bg-blue-100 dark:bg-blue-900/30 dark:text-blue-400 dark:hover:bg-blue-900/50"
                  onClick={() => handleCopy(payload().result.translated)}
                >
                  {copied() ? "已复制" : "复制"}
                </button>
              </div>
            </div>
          )}
        </Show>
      </Show>
    </div>
  );
}

function CloseButton(props: { onClick: () => void }) {
  return (
    <button
      type="button"
      class="grid h-6 w-6 cursor-pointer place-items-center rounded-md text-gray-400 hover:bg-gray-100 hover:text-gray-700 dark:hover:bg-gray-800 dark:hover:text-gray-200"
      aria-label="关闭"
      onClick={() => props.onClick()}
    >
      ×
    </button>
  );
}
