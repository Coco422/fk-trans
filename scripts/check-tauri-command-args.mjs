import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";

const root = dirname(dirname(fileURLToPath(import.meta.url)));
const appSource = readFileSync(join(root, "src", "App.tsx"), "utf8");
const argsSource = readFileSync(join(root, "src", "tauriArgs.ts"), "utf8");
const ocrOverlaySource = readFileSync(
  join(root, "src", "components", "OcrSelectionOverlay.tsx"),
  "utf8"
);
const popupSource = readFileSync(
  join(root, "src", "components", "FloatingPopup.tsx"),
  "utf8"
);

assert.match(
  appSource,
  /invoke\("update_provider",\s*buildUpdateProviderArgs\(name,\s*draft\)\)/,
  "update_provider must use the typed Tauri argument builder"
);

for (const key of ["baseUrl", "apiKey", "systemPrompt", "userPrompt", "extraParams"]) {
  assert.match(argsSource, new RegExp(`\\b${key}:`), `missing camelCase key ${key}`);
}

assert.doesNotMatch(
  appSource,
  /invoke\("update_provider",\s*\{[\s\S]*?base_url:/,
  "update_provider cannot pass snake_case keys to Tauri"
);

assert.match(
  ocrOverlaySource,
  /invoke\("complete_ocr_selection",\s*\{[\s\S]*?sessionId:/,
  "complete_ocr_selection must pass camelCase sessionId"
);

assert.doesNotMatch(
  ocrOverlaySource,
  /invoke\("complete_ocr_selection",\s*\{[\s\S]*?session_id:/,
  "complete_ocr_selection cannot pass snake_case session_id"
);

assert.match(
  appSource,
  /invoke<MacosDevPermissionTarget>\(\s*"get_macos_dev_permission_target"\s*\)/,
  "Diagnostics must invoke get_macos_dev_permission_target without args"
);

assert.match(
  appSource,
  /invoke\("reveal_current_executable"\)/,
  "Diagnostics must expose reveal_current_executable"
);

assert.match(
  popupSource,
  /tabIndex=\{0\}/,
  "FloatingPopup root must be focusable for Escape handling"
);

assert.match(
  popupSource,
  /window\.addEventListener\("keydown",\s*handleKeydown\)/,
  "FloatingPopup must listen for window keydown"
);

assert.match(
  popupSource,
  /e\.key === "Escape"[\s\S]*hidePopup\(\)/,
  "FloatingPopup Escape handler must hide the popup"
);
