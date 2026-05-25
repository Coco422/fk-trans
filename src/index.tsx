/* @refresh reload */
import { render } from "solid-js/web";
import { Router, Route } from "@solidjs/router";
import { invoke } from "@tauri-apps/api/core";
import App from "./App";
import FloatingPopup from "./components/FloatingPopup";
import OcrSelectionOverlay from "./components/OcrSelectionOverlay";
import "./styles/globals.css";

function logFrontendEvent(
  level: "error" | "warn" | "info",
  message: string,
  context?: Record<string, unknown>
) {
  invoke("log_frontend_event", { level, message, context }).catch(() => {});
}

window.addEventListener("error", (event) => {
  logFrontendEvent("error", event.message || "window error", {
    filename: event.filename,
    lineno: event.lineno,
    colno: event.colno,
  });
});

window.addEventListener("unhandledrejection", (event) => {
  logFrontendEvent("error", "unhandled promise rejection", {
    reason:
      event.reason instanceof Error
        ? event.reason.message
        : String(event.reason ?? "unknown"),
  });
});

render(
  () => (
    <Router>
      <Route path="/" component={App} />
      <Route path="/popup" component={FloatingPopup} />
      <Route path="/ocr-select" component={OcrSelectionOverlay} />
    </Router>
  ),
  document.getElementById("root") as HTMLElement
);
