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
const libSource = readFileSync(join(root, "src-tauri", "src", "lib.rs"), "utf8");
const globalsSource = readFileSync(join(root, "src", "styles", "globals.css"), "utf8");
const popupSizingStart = libSource.indexOf("fn show_popup_at_cursor_with_size");
const popupSizingEnd = libSource.indexOf("pub(crate) fn show_popup_at_cursor");
const popupSizingSource =
  popupSizingStart >= 0 && popupSizingEnd > popupSizingStart
    ? libSource.slice(popupSizingStart, popupSizingEnd)
    : "";

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

assert.match(
  popupSource,
  /listen<OcrPayload>\("ocr-ready"/,
  "FloatingPopup must listen for standalone OCR results"
);

assert.match(
  popupSource,
  /invoke<TranslateResult>\("translate_text",\s*\{[\s\S]*?from:[\s\S]*?to:/,
  "OCR manual translation must call translate_text with Rust command keys from/to"
);

assert.match(
  popupSource,
  /new Channel<AiActionStreamEvent>\(\)/,
  "FloatingPopup must create a Tauri channel for streaming AI actions"
);

assert.match(
  popupSource,
  /invoke<void>\("ai_action_stream",\s*\{[\s\S]*?\bsourceLang\b[\s\S]*?\btargetLang\b[\s\S]*?\bonEvent\b/,
  "ai_action_stream must pass camelCase sourceLang, targetLang, and onEvent"
);

assert.doesNotMatch(
  popupSource,
  /await\s+invoke<void>\("ai_action_stream"/,
  "ai_action_stream must be fire-and-forget so popup close remains responsive"
);

assert.match(
  popupSource,
  /invoke<void>\("ai_action_stream",\s*\{[\s\S]*?\}\)\.catch/,
  "ai_action_stream startup errors must be handled without awaiting the command"
);

assert.match(
  popupSource,
  /event\.type === "done"[\s\S]*setActionLoading\(null\)/,
  "FloatingPopup must clear AI action loading when stream sends done"
);

assert.match(
  popupSource,
  /function hidePopup\(\)[\s\S]*cancelActiveAction\(\)[\s\S]*clearPopupContent\(\)[\s\S]*getCurrentWindow\(\)\.hide/,
  "Closing the popup must clear local popup state before hiding"
);

assert.doesNotMatch(
  popupSource,
  /invoke<void>\("ai_action_stream",\s*\{[\s\S]*?source_lang:/,
  "ai_action_stream cannot pass snake_case source_lang"
);

assert.match(
  popupSource,
  /when=\{actionLoading\(\)\}[\s\S]*fallback=\{<MarkdownView source=\{extraResult\(\) \|\| "…"\} \/>[\s\S]*whitespace-pre-wrap/,
  "Streaming AI action output must stay plain text until Markdown renders after completion"
);

assert.match(
  globalsSource,
  /html,\s*body,\s*#root\s*\{[\s\S]*width:\s*100%;[\s\S]*height:\s*100%;[\s\S]*overflow:\s*hidden;/,
  "Popup height math requires html, body, and #root to have explicit full size"
);

assert.match(
  globalsSource,
  /\.popup-content-scroll\s*\{[\s\S]*flex:\s*1 1 auto;[\s\S]*min-height:\s*0;[\s\S]*overflow-y:\s*auto;[\s\S]*overscroll-behavior:\s*contain;[\s\S]*contain:\s*layout paint;/,
  "Popup content must use a single bounded scrolling container"
);

assert.match(
  popupSource,
  /const popupContentClass =[\s\S]*popup-content-scroll/,
  "FloatingPopup must centralize its body scroller class"
);

assert.doesNotMatch(
  popupSource,
  /max-h-full/,
  "FloatingPopup must avoid max-h-full in the fixed-size popup height chain"
);

assert.doesNotMatch(
  popupSource,
  /flex-1\s+overflow-y-auto/,
  "FloatingPopup must avoid scattered flex-1 overflow-y-auto body scrollers"
);

assert.match(
  popupSizingSource,
  /tauri::Size::Logical/,
  "Popup windows must be sized with logical pixels"
);

assert.doesNotMatch(
  popupSizingSource,
  /tauri::Size::Physical/,
  "Popup windows must not be sized with physical pixels"
);

assert.match(
  libSource,
  /commands::translation::ai_action_stream/,
  "ai_action_stream must be registered in generate_handler"
);

assert.doesNotMatch(
  popupSource,
  /invoke<TranslateResult>\("translate_text",\s*\{[\s\S]*?sourceLang:/,
  "translate_text cannot receive sourceLang"
);

assert.match(
  appSource,
  /saveConfig\(\{\s*action_prompts:\s*draft\s*\}\)/,
  "Settings must save editable AI action prompts"
);
