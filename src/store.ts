/**
 * The state the Settings window shares between its tabs.
 *
 * A single module-level store rather than props threaded through the tree,
 * because every tab needs the same three things — the config, the history and
 * the diagnostics — and the window has exactly one instance of each. The
 * providers tab keeps its own draft state, which nothing outside it can use.
 *
 * It lives in a `createRoot` because a resource created outside one leaks its
 * reactive scope: nothing would ever dispose it, and Solid warns.
 */

import { listen } from "@tauri-apps/api/event";
import { createResource, createRoot, createSignal } from "solid-js";
import * as api from "./api/commands";
import type {
  AppConfig,
  ConfigPatch,
  DiagnosticsSnapshot,
  HistoryEntry,
  MouseTriggerState,
} from "./api/types";
import { applyTheme, setTheme, storedTheme, type Theme } from "./theme";

export type TabId = "general" | "providers" | "diagnostics" | "history";

export const TABS: ReadonlyArray<{ id: TabId; label: string }> = [
  { id: "general", label: "通用" },
  { id: "providers", label: "翻译服务" },
  { id: "diagnostics", label: "诊断" },
  { id: "history", label: "历史" },
];

/** Turn an unknown thrown value into something worth showing in the UI. */
export function describeError(error: unknown): string {
  if (error instanceof Error) {
    return error.message;
  }
  return typeof error === "string" ? error : JSON.stringify(error);
}

function createStore() {
  const [config, { mutate }] = createResource<AppConfig>(() => api.getConfig());
  const [history, setHistory] = createSignal<HistoryEntry[]>([]);
  const [diagnostics, setDiagnostics] = createSignal<DiagnosticsSnapshot | null>(null);
  const [message, setMessage] = createSignal("");
  const [tab, setTab] = createSignal<TabId>("general");
  const [theme, setThemeSignal] = createSignal<Theme>(storedTheme());

  async function reloadConfig() {
    try {
      mutate(await api.getConfig());
    } catch (error) {
      console.error("failed to reload config", error);
    }
  }

  async function refreshDiagnostics() {
    try {
      setDiagnostics(await api.getDiagnosticsSnapshot());
    } catch (error) {
      console.error("failed to read diagnostics", error);
      setDiagnostics(null);
    }
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

  async function startMouseTest() {
    setMessage("请在 10 秒内按下鼠标中键。");
    try {
      const mouse = await api.startMiddleClickTest();
      setDiagnostics((prev) => (prev ? { ...prev, mouse } : prev));
    } catch (error) {
      setMessage(`启动检测失败：${describeError(error)}`);
    }
  }

  async function saveLastMouseButton() {
    const button = diagnostics()?.mouse.lastButton;
    if (button === undefined || button === null) {
      setMessage("还没有检测到任何按键，请先开始检测。");
      return;
    }
    try {
      await saveConfig({ mouseTriggerButton: button });
      setMessage(`已把按键 ${button} 设为触发键。`);
    } catch (error) {
      setMessage(`保存触发键失败：${describeError(error)}`);
    }
  }

  async function openAccessibilitySettings() {
    try {
      await api.openAccessibilitySettings();
      setMessage("已打开 macOS 辅助功能设置。");
    } catch (error) {
      setMessage(describeError(error));
    }
  }

  async function exportDiagnostics() {
    try {
      const result = await api.exportDiagnosticsReport();
      setMessage(`诊断报告已导出：${result.path}`);
    } catch (error) {
      setMessage(`导出失败：${describeError(error)}`);
    }
  }

  async function revealDiagnosticsFolder() {
    try {
      await api.revealDiagnosticsFolder();
    } catch (error) {
      setMessage(`打开目录失败：${describeError(error)}`);
    }
  }

  function chooseTheme(next: Theme) {
    setTheme(next);
    setThemeSignal(next);
  }

  /**
   * Follow the events Rust already sends instead of asking it the same
   * questions on a timer.
   *
   * The diagnostics panel used to be refreshed once a second, which was a
   * command and a full snapshot every second to catch changes that arrive as
   * events anyway. The two things that are not events — the accessibility
   * permission, which the user can change in another app's window — are picked
   * up when this window is brought forward, which is when they would be looked
   * at.
   */
  function start() {
    applyTheme(storedTheme());

    const stops: Array<() => void> = [];

    listen("config-changed", () => void reloadConfig()).then((stop) => stops.push(stop));

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
    }).then((stop) => stops.push(stop));

    window.addEventListener("focus", refreshDiagnostics);

    return () => {
      for (const stop of stops) {
        stop();
      }
      window.removeEventListener("focus", refreshDiagnostics);
    };
  }

  return {
    config,
    reloadConfig,
    saveConfig,
    history,
    loadHistory,
    clearHistory,
    diagnostics,
    refreshDiagnostics,
    message,
    setMessage,
    tab,
    setTab,
    theme,
    chooseTheme,
    startMouseTest,
    saveLastMouseButton,
    openAccessibilitySettings,
    exportDiagnostics,
    revealDiagnosticsFolder,
    start,
  };
}

export const store = createRoot(createStore);
