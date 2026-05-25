export interface TranslateResult {
  original: string;
  translated: string;
  sourceLang?: string;
  targetLang?: string;
  source_lang?: string;
  target_lang?: string;
  provider: string;
  alternatives: string[];
}

export interface TranslationPayload {
  original: string;
  result: TranslateResult;
  cursor_x: number;
  cursor_y: number;
  capture_source?: "clipboard" | "ocr";
  ocr_backend?: "apple_vision";
  ocr_elapsed_ms?: number;
}

export type AiActionStreamEvent =
  | { type: "delta"; text: string }
  | { type: "done" }
  | { type: "error"; message: string };

export interface OcrTextRegion {
  text: string;
  x: number;
  y: number;
  width: number;
  height: number;
}

export interface OcrPayload {
  text: string;
  imageDataUrl: string;
  regions: OcrTextRegion[];
  cursorX: number;
  cursorY: number;
  captureSource: "ocr";
  ocrBackend: "apple_vision";
  ocrElapsedMs: number;
  sourceLang: string;
  targetLang: string;
}

export interface ProviderConfig {
  name: string;
  base_url: string;
  api_key: string;
  model: string;
  system_prompt: string;
  user_prompt: string;
  extra_params: Record<string, unknown>;
}

export interface ActionPromptConfig {
  explain: string;
  summary: string;
  polish: string;
  dict: string;
}

export interface AppConfig {
  enabled: boolean;
  debug_logging: boolean;
  ocr_enabled: boolean;
  selection_trigger_enabled: boolean;
  source_lang: string;
  target_lang: string;
  active_provider: string;
  mouse_trigger_button: number;
  action_prompts: ActionPromptConfig;
  providers: ProviderConfig[];
}
