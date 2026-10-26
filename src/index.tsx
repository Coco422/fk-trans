/* @refresh reload */
import { render } from "solid-js/web";
import App from "./App";
import { logFrontendEvent } from "./api/commands";
import FloatingPopup from "./components/FloatingPopup";
import "./styles/globals.css";
import { startTheme } from "./theme";

function report(
  level: "error" | "warn" | "info",
  message: string,
  context?: Record<string, unknown>
) {
  // Fire and forget: a failure to log must never become a second failure.
  void logFrontendEvent({ level, message, context }).catch(() => {});
}

window.addEventListener("error", (event) => {
  report("error", event.message || "window error", {
    filename: event.filename,
    lineno: event.lineno,
    colno: event.colno,
  });
});

window.addEventListener("unhandledrejection", (event) => {
  report("error", "unhandled promise rejection", {
    reason:
      event.reason instanceof Error
        ? event.reason.message
        : String(event.reason ?? "unknown"),
  });
});

/**
 * Which window this is.
 *
 * The popup window is declared with `"url": "/popup"` in `tauri.conf.json`, so
 * the pathname is enough. This used to go through `@solidjs/router` — a 57 KB
 * dependency, used for a single two-way branch, in a window that never
 * navigates anywhere.
 */
const stopTheme = startTheme();
window.addEventListener("beforeunload", stopTheme, { once: true });

function isPopupWindow(): boolean {
  return window.location.pathname.startsWith("/popup");
}

render(
  () => (isPopupWindow() ? <FloatingPopup /> : <App />),
  document.getElementById("root") as HTMLElement
);
