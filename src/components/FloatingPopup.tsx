import {
  createMemo,
  createSignal,
  onCleanup,
  onMount,
  Show,
  For,
  type JSX,
} from "solid-js";
import { listen } from "@tauri-apps/api/event";
import { Channel, invoke } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
import type {
  AiActionStreamEvent,
  OcrPayload,
  OcrTextRegion,
  TranslateResult,
  TranslationPayload,
} from "../types/translation";
import MarkdownView from "./MarkdownView";

const ACTIONS = [
  { id: "explain", label: "Explain" },
  { id: "summary", label: "Summary" },
  { id: "polish", label: "Polish" },
  { id: "dict", label: "Dictionary" },
] as const;

type ActionId = (typeof ACTIONS)[number]["id"];
type LoadingKind = "translation" | "ocr";

const cardClass =
  "bg-white/95 dark:bg-gray-900/95 backdrop-blur-md rounded-lg shadow-sm border border-gray-200/80 dark:border-gray-800/80 w-full h-full min-h-0 flex flex-col overflow-hidden";
const iconButtonClass =
  "w-6 h-6 grid place-items-center rounded-md text-gray-400 hover:text-gray-700 hover:bg-gray-100 dark:hover:text-gray-200 dark:hover:bg-gray-800 cursor-pointer";
const subtleButtonClass =
  "text-xs px-2.5 py-1.5 rounded-md bg-gray-100 dark:bg-gray-800 hover:bg-gray-200 dark:hover:bg-gray-700 transition-colors cursor-pointer disabled:opacity-50 disabled:cursor-not-allowed";
const popupRootClass =
  "w-full h-full min-h-0 flex items-stretch justify-stretch p-3 m-0 overflow-hidden outline-none text-gray-900 dark:text-gray-100";
const popupContentClass = "popup-scroll popup-content-scroll px-4 py-3";

export default function FloatingPopup() {
  const [data, setData] = createSignal<TranslationPayload | null>(null);
  const [ocrData, setOcrData] = createSignal<OcrPayload | null>(null);
  const [ocrTranslation, setOcrTranslation] = createSignal<TranslateResult | null>(null);
  const [loadingKind, setLoadingKind] = createSignal<LoadingKind | null>(null);
  const [error, setError] = createSignal<string | null>(null);
  const [extraResult, setExtraResult] = createSignal<string>("");
  const [extraTitle, setExtraTitle] = createSignal<string>("");
  const [copied, setCopied] = createSignal(false);
  const [actionLoading, setActionLoading] = createSignal<string | null>(null);
  const [ocrTranslating, setOcrTranslating] = createSignal(false);
  const [selectedRegionIndex, setSelectedRegionIndex] = createSignal<number | null>(null);

  let hideTimeout: ReturnType<typeof setTimeout> | undefined;
  let root: HTMLDivElement | undefined;
  let actionRunSerial = 0;

  const currentText = createMemo(() => data()?.original ?? ocrData()?.text ?? "");

  function cancelActiveAction() {
    actionRunSerial += 1;
    setActionLoading(null);
  }

  function clearPopupContent() {
    setData(null);
    setOcrData(null);
    setOcrTranslation(null);
    setLoadingKind(null);
    setError(null);
    setExtraResult("");
    setExtraTitle("");
    setOcrTranslating(false);
    setSelectedRegionIndex(null);
  }

  function resetHideTimeout() {
    if (hideTimeout) clearTimeout(hideTimeout);
    hideTimeout = setTimeout(() => {
      hidePopup();
    }, 45_000);
  }

  function focusPopupRoot() {
    queueMicrotask(() => root?.focus());
  }

  onMount(async () => {
    const win = getCurrentWindow();

    const unlisten = await listen<TranslationPayload>(
      "translation-ready",
      (event) => {
        cancelActiveAction();
        setData(event.payload);
        setOcrData(null);
        setOcrTranslation(null);
        setExtraResult("");
        setExtraTitle("");
        setError(null);
        setLoadingKind(null);
        focusPopupRoot();
        resetHideTimeout();
      }
    );

    const unlistenOcr = await listen<OcrPayload>("ocr-ready", (event) => {
      cancelActiveAction();
      setOcrData(event.payload);
      setData(null);
      setOcrTranslation(null);
      setSelectedRegionIndex(null);
      setExtraResult("");
      setExtraTitle("");
      setError(null);
      setLoadingKind(null);
      focusPopupRoot();
      resetHideTimeout();
    });

    const unlistenLoading = await listen("translation-started", () => {
      cancelActiveAction();
      setLoadingKind("translation");
      setData(null);
      setOcrData(null);
      setOcrTranslation(null);
      setError(null);
      setExtraResult("");
      setExtraTitle("");
      focusPopupRoot();
    });

    const unlistenOcrLoading = await listen("ocr-started", () => {
      cancelActiveAction();
      setLoadingKind("ocr");
      setData(null);
      setOcrData(null);
      setOcrTranslation(null);
      setError(null);
      setExtraResult("");
      setExtraTitle("");
      focusPopupRoot();
    });

    const unlistenError = await listen<string>("translation-error", (event) => {
      cancelActiveAction();
      setError(event.payload);
      setLoadingKind(null);
      setData(null);
      setOcrData(null);
      setOcrTranslation(null);
      focusPopupRoot();
    });

    const unlistenBlur = await win.onCloseRequested(() => {
      // prevent close, just hide
    });

    window.addEventListener("keydown", handleKeydown);
    document.addEventListener("keydown", handleKeydown);

    onCleanup(() => {
      unlisten();
      unlistenOcr();
      unlistenLoading();
      unlistenOcrLoading();
      unlistenError();
      unlistenBlur();
      window.removeEventListener("keydown", handleKeydown);
      document.removeEventListener("keydown", handleKeydown);
      if (hideTimeout) clearTimeout(hideTimeout);
    });
  });

  function handleKeydown(e: KeyboardEvent) {
    if (e.key === "Escape") {
      e.preventDefault();
      e.stopPropagation();
      hidePopup();
    }
  }

  function hidePopup() {
    if (hideTimeout) clearTimeout(hideTimeout);
    cancelActiveAction();
    clearPopupContent();
    getCurrentWindow().hide().catch((e) => setError(`Close failed: ${e}`));
  }

  function startDrag(e: MouseEvent) {
    if (e.button !== 0) return;
    getCurrentWindow().startDragging().catch(() => {});
  }

  function actionLabel(actionId: string) {
    return ACTIONS.find((action) => action.id === actionId)?.label ?? actionId;
  }

  function actionLanguages() {
    const payload = data();
    if (payload) {
      return {
        sourceLang: payload.result.sourceLang ?? payload.result.source_lang ?? "auto",
        targetLang: payload.result.targetLang ?? payload.result.target_lang ?? "zh",
      };
    }

    const ocr = ocrData();
    return {
      sourceLang: ocr?.sourceLang ?? "auto",
      targetLang: ocr?.targetLang ?? "zh",
    };
  }

  function handleAction(action: ActionId) {
    const text = currentText().trim();
    if (!text) return;
    const { sourceLang, targetLang } = actionLanguages();

    setActionLoading(action);
    setExtraResult("");
    setExtraTitle(actionLabel(action));
    const runId = ++actionRunSerial;
    let pendingText = "";
    let flushTimer: ReturnType<typeof setTimeout> | undefined;
    const flushPendingText = () => {
      if (runId !== actionRunSerial || !pendingText) return;
      const text = pendingText;
      pendingText = "";
      setExtraResult((prev) => prev + text);
      resetHideTimeout();
    };
    const scheduleFlush = () => {
      if (flushTimer) return;
      flushTimer = setTimeout(() => {
        flushTimer = undefined;
        flushPendingText();
      }, 200);
    };

    const onEvent = new Channel<AiActionStreamEvent>();
    onEvent.onmessage = (event) => {
      if (runId !== actionRunSerial) return;
      if (event.type === "delta") {
        pendingText += event.text;
        scheduleFlush();
      } else if (event.type === "done") {
        if (flushTimer) {
          clearTimeout(flushTimer);
          flushTimer = undefined;
        }
        flushPendingText();
        setActionLoading(null);
        resetHideTimeout();
      } else if (event.type === "error") {
        if (flushTimer) {
          clearTimeout(flushTimer);
          flushTimer = undefined;
        }
        flushPendingText();
        setExtraResult((prev) =>
          prev ? `${prev}\n\nError: ${event.message}` : `Error: ${event.message}`
        );
        setActionLoading(null);
      }
    };

    invoke<void>("ai_action_stream", {
      text,
      action,
      sourceLang,
      targetLang,
      onEvent,
    }).catch((e) => {
      if (runId === actionRunSerial) {
        setExtraResult((prev) => (prev ? prev : `Error: ${e}`));
        setActionLoading(null);
      }
    });
  }

  async function translateOcrText() {
    const payload = ocrData();
    if (!payload?.text.trim()) return;

    setOcrTranslating(true);
    setExtraResult("");
    setExtraTitle("");
    try {
      const result = await invoke<TranslateResult>("translate_text", {
        text: payload.text,
        from: payload.sourceLang || "auto",
        to: payload.targetLang || "zh",
      });
      setOcrTranslation(result);
      resetHideTimeout();
    } catch (e) {
      setExtraTitle("Translate");
      setExtraResult(`Error: ${e}`);
    } finally {
      setOcrTranslating(false);
    }
  }

  async function handleCopy(text: string) {
    try {
      await navigator.clipboard.writeText(text);
      setCopied(true);
      setTimeout(() => setCopied(false), 1500);
    } catch {
      await invoke("plugin:clipboard-manager|write_text", { text });
      setCopied(true);
      setTimeout(() => setCopied(false), 1500);
    }
  }

  function regionStyle(region: OcrTextRegion) {
    return {
      left: `${region.x * 100}%`,
      top: `${region.y * 100}%`,
      width: `${region.width * 100}%`,
      height: `${region.height * 100}%`,
    };
  }

  function CloseButton() {
    return (
      <button
        class={iconButtonClass}
        aria-label="Close"
        onMouseDown={(e) => e.stopPropagation()}
        onClick={(e) => {
          e.stopPropagation();
          hidePopup();
        }}
      >
        x
      </button>
    );
  }

  function ActionButtons() {
    return (
      <For each={ACTIONS}>
        {(action) => (
          <button
            class={subtleButtonClass}
            title={action.id === "dict" ? "Dictionary-style lookup" : action.label}
            disabled={actionLoading() !== null || !currentText().trim()}
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
    );
  }

  function PopupShell(props: {
    animationClass: string;
    header: JSX.Element;
    footer?: JSX.Element;
    children: JSX.Element;
  }) {
    return (
      <div class={`${cardClass} ${props.animationClass}`}>
        {props.header}
        <div class={popupContentClass}>{props.children}</div>
        {props.footer}
      </div>
    );
  }

  function ExtraResult() {
    return (
      <Show when={extraResult() || actionLoading()}>
        <div class="mt-3 pt-3 border-t border-gray-200 dark:border-gray-800">
          <Show when={extraTitle()}>
            <div class="text-[11px] uppercase tracking-wide text-gray-400 mb-1">
              {extraTitle()}
            </div>
          </Show>
          <Show
            when={actionLoading()}
            fallback={<MarkdownView source={extraResult() || "…"} />}
          >
            <div class="text-sm text-gray-700 dark:text-gray-200 whitespace-pre-wrap leading-relaxed">
              {extraResult() || "…"}
            </div>
          </Show>
        </div>
      </Show>
    );
  }

  function OcrComparison(props: { payload: OcrPayload }) {
    return (
      <div class="space-y-3">
        <div class="relative overflow-hidden rounded-md border border-gray-200 dark:border-gray-800 bg-gray-100 dark:bg-gray-950">
          <img
            src={props.payload.imageDataUrl}
            alt="OCR selection"
            class="block w-full h-auto select-none"
            draggable={false}
          />
          <For each={props.payload.regions}>
            {(region, index) => (
              <button
                class={`absolute border transition-colors ${
                  selectedRegionIndex() === index()
                    ? "border-blue-500 bg-blue-500/20"
                    : "border-blue-400/80 bg-blue-400/10 hover:bg-blue-400/20"
                }`}
                style={regionStyle(region)}
                title={region.text}
                onMouseEnter={() => setSelectedRegionIndex(index())}
                onFocus={() => setSelectedRegionIndex(index())}
                onClick={() => setSelectedRegionIndex(index())}
              />
            )}
          </For>
        </div>

        <Show when={props.payload.regions.length > 0}>
          <div class="space-y-1">
            <For each={props.payload.regions}>
              {(region, index) => (
                <button
                  class={`w-full text-left text-xs px-2 py-1.5 rounded border transition-colors ${
                    selectedRegionIndex() === index()
                      ? "border-blue-300 bg-blue-50 text-blue-700 dark:border-blue-700 dark:bg-blue-950/40 dark:text-blue-200"
                      : "border-gray-200 bg-gray-50 text-gray-600 hover:bg-gray-100 dark:border-gray-800 dark:bg-gray-950 dark:text-gray-300 dark:hover:bg-gray-800"
                  }`}
                  onMouseEnter={() => setSelectedRegionIndex(index())}
                  onFocus={() => setSelectedRegionIndex(index())}
                  onClick={() => setSelectedRegionIndex(index())}
                >
                  <span class="mr-2 font-mono text-gray-400">
                    {String(index() + 1).padStart(2, "0")}
                  </span>
                  {region.text}
                </button>
              )}
            </For>
          </div>
        </Show>
      </div>
    );
  }

  return (
    <div
      ref={root}
      tabIndex={0}
      class={popupRootClass}
      onMouseDown={focusPopupRoot}
    >
      <Show when={error()}>
        <PopupShell
          animationClass="animate-fade-in"
          header={
            <div
              class="shrink-0 flex items-center gap-2 text-red-500 px-4 py-3 cursor-move select-none border-b border-gray-200 dark:border-gray-800"
              onMouseDown={startDrag}
            >
              <span class="text-sm font-medium">fk-trans Error</span>
              <div class="ml-auto">
                <CloseButton />
              </div>
            </div>
          }
          footer={
            <div class="shrink-0 px-4 py-3 border-t border-gray-200 dark:border-gray-800">
              <button
                class="text-xs px-3 py-1.5 rounded-md bg-red-50 dark:bg-red-900/30 text-red-600 dark:text-red-400 hover:bg-red-100 dark:hover:bg-red-900/50 transition-colors cursor-pointer"
                onClick={hidePopup}
              >
                Dismiss
              </button>
            </div>
          }
        >
            <p class="text-xs text-gray-600 dark:text-gray-400 whitespace-pre-wrap">
              {error()}
            </p>
        </PopupShell>
      </Show>

      <Show when={loadingKind() && !data() && !ocrData() && !error()}>
        <PopupShell
          animationClass="animate-fade-in"
          header={
            <div
              class="shrink-0 flex items-center gap-3 px-4 py-3 cursor-move select-none"
              onMouseDown={startDrag}
            >
              <div class="relative w-5 h-5">
                <div class="absolute inset-0 rounded-full border-2 border-blue-200 dark:border-blue-800" />
                <div class="absolute inset-0 rounded-full border-2 border-transparent border-t-blue-500 animate-spin" />
              </div>
              <span class="text-sm font-medium text-gray-700 dark:text-gray-300">
                {loadingKind() === "ocr" ? "Reading text..." : "Translating..."}
              </span>
              <div class="ml-auto">
                <CloseButton />
              </div>
            </div>
          }
        >
          <div class="space-y-2">
            <div class="h-3 w-3/4 rounded bg-gradient-to-r from-gray-200 via-gray-100 to-gray-200 dark:from-gray-700 dark:via-gray-600 dark:to-gray-700 animate-pulse" />
            <div class="h-3 w-1/2 rounded bg-gradient-to-r from-gray-200 via-gray-100 to-gray-200 dark:from-gray-700 dark:via-gray-600 dark:to-gray-700 animate-pulse" />
          </div>
        </PopupShell>
      </Show>

      <Show when={data()}>
        {(payload) => (
          <PopupShell
            animationClass="animate-result-in"
            header={
              <div
                class="shrink-0 flex items-center justify-between gap-3 px-4 py-3 cursor-move select-none border-b border-gray-200 dark:border-gray-800"
                onMouseDown={startDrag}
              >
                <div class="text-xs text-gray-500 dark:text-gray-400 font-mono truncate">
                  {payload().result.sourceLang ?? payload().result.source_lang} to{" "}
                  {payload().result.targetLang ?? payload().result.target_lang}
                </div>
                <div class="flex items-center gap-2">
                  <Show when={payload().capture_source === "ocr"}>
                    <span class="text-[10px] px-1.5 py-0.5 rounded bg-blue-50 dark:bg-blue-900/30 text-blue-600 dark:text-blue-300">
                      OCR
                    </span>
                  </Show>
                  <span class="text-[10px] px-1.5 py-0.5 rounded bg-gray-100 dark:bg-gray-800 text-gray-500 dark:text-gray-400">
                    {payload().result.provider}
                  </span>
                  <CloseButton />
                </div>
              </div>
            }
            footer={
              <div class="shrink-0 flex flex-wrap gap-1.5 px-4 py-3 border-t border-gray-200 dark:border-gray-800">
                <ActionButtons />
                <button
                  class="text-xs px-2.5 py-1.5 rounded-md bg-blue-50 dark:bg-blue-900/30 text-blue-600 dark:text-blue-400 hover:bg-blue-100 dark:hover:bg-blue-900/50 transition-colors ml-auto cursor-pointer"
                  onClick={() => handleCopy(payload().result.translated)}
                >
                  {copied() ? "Copied" : "Copy"}
                </button>
              </div>
            }
          >
              <div class="text-[11px] uppercase tracking-wide text-gray-400 mb-1">
                Original
              </div>
              <div class="text-sm text-gray-500 dark:text-gray-400 mb-3 pb-3 border-b border-gray-200 dark:border-gray-800 whitespace-pre-wrap leading-relaxed">
                {payload().original}
              </div>

              <div class="text-[11px] uppercase tracking-wide text-gray-400 mb-1">
                Translation
              </div>
              <div class="text-base text-gray-900 dark:text-white whitespace-pre-wrap leading-relaxed">
                {payload().result.translated}
              </div>

              <ExtraResult />
          </PopupShell>
        )}
      </Show>

      <Show when={ocrData()}>
        {(payload) => (
          <PopupShell
            animationClass="animate-result-in"
            header={
              <div
                class="shrink-0 flex items-center justify-between gap-3 px-4 py-3 cursor-move select-none border-b border-gray-200 dark:border-gray-800"
                onMouseDown={startDrag}
              >
                <div class="flex items-center gap-2 min-w-0">
                  <span class="text-[10px] px-1.5 py-0.5 rounded bg-blue-50 dark:bg-blue-900/30 text-blue-600 dark:text-blue-300">
                    OCR
                  </span>
                  <span class="text-xs text-gray-500 dark:text-gray-400 truncate">
                    {payload().ocrBackend} · {payload().ocrElapsedMs} ms
                  </span>
                </div>
                <CloseButton />
              </div>
            }
            footer={
              <div class="shrink-0 flex flex-wrap gap-1.5 px-4 py-3 border-t border-gray-200 dark:border-gray-800">
                <button
                  class="text-xs px-2.5 py-1.5 rounded-md bg-blue-600 text-white hover:bg-blue-700 transition-colors cursor-pointer disabled:opacity-60 disabled:cursor-not-allowed"
                  disabled={ocrTranslating()}
                  onClick={translateOcrText}
                >
                  {ocrTranslating() ? "Translating..." : "Translate"}
                </button>
                <ActionButtons />
                <button
                  class={subtleButtonClass}
                  onClick={() => handleCopy(ocrTranslation()?.translated ?? payload().text)}
                >
                  {copied() ? "Copied" : ocrTranslation() ? "Copy Translation" : "Copy Text"}
                </button>
              </div>
            }
          >
              <OcrComparison payload={payload()} />

              <div class="mt-3 pt-3 border-t border-gray-200 dark:border-gray-800">
                <div class="text-[11px] uppercase tracking-wide text-gray-400 mb-1">
                  Extracted Text
                </div>
                <div class="text-sm text-gray-800 dark:text-gray-100 whitespace-pre-wrap leading-relaxed">
                  {payload().text}
                </div>
              </div>

              <Show when={ocrTranslation()}>
                {(result) => (
                  <div class="mt-3 pt-3 border-t border-gray-200 dark:border-gray-800">
                    <div class="flex items-center justify-between gap-2 mb-1">
                      <div class="text-[11px] uppercase tracking-wide text-gray-400">
                        Translation
                      </div>
                      <span class="text-[10px] px-1.5 py-0.5 rounded bg-gray-100 dark:bg-gray-800 text-gray-500 dark:text-gray-400">
                        {result().provider}
                      </span>
                    </div>
                    <div class="text-base text-gray-900 dark:text-white whitespace-pre-wrap leading-relaxed">
                      {result().translated}
                    </div>
                  </div>
                )}
              </Show>

              <ExtraResult />
          </PopupShell>
        )}
      </Show>
    </div>
  );
}
