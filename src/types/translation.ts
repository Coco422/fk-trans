export interface TranslateResult {
  original: string;
  translated: string;
  sourceLang?: string;
  targetLang?: string;
  source_lang: string;
  target_lang: string;
  provider: string;
  alternatives: string[];
}

export interface TranslationPayload {
  original: string;
  result: TranslateResult;
  cursor_x: number;
  cursor_y: number;
}

export interface ProviderConfig {
  name: string;
  base_url: string;
  api_key: string;
  model: string;
}

export interface AppConfig {
  enabled: boolean;
  source_lang: string;
  target_lang: string;
  active_provider: string;
  providers: ProviderConfig[];
}
