/**
 * Chinese labels for the language codes.
 *
 * The *set* of languages comes from Rust, which is the only place that knows
 * which ones a prompt can be built for. What each is called on screen is this
 * window's business, so it is here — and a code with no entry falls back to the
 * English name rather than disappearing from the list.
 */
const LABELS: Record<string, string> = {
  auto: "自动检测",
  en: "英语",
  zh: "中文",
  ja: "日语",
  ko: "韩语",
  fr: "法语",
  de: "德语",
  es: "西班牙语",
  ru: "俄语",
  pt: "葡萄牙语",
  ar: "阿拉伯语",
  it: "意大利语",
  nl: "荷兰语",
  pl: "波兰语",
  tr: "土耳其语",
  vi: "越南语",
  th: "泰语",
};

export function languageLabel(code: string): string {
  return LABELS[code] ?? code;
}

/**
 * What each provider is called on screen.
 *
 * A name with no entry is shown as it is: a provider added in Rust should still
 * appear here, even before anyone has written a label for it.
 */
const PROVIDERS: Record<string, string> = {
  deeplx: "DeepLX（本地）",
  openai: "OpenAI 兼容",
  gemini: "Google Gemini",
  claude: "Anthropic Claude",
  ollama: "Ollama（本地）",
};

export function providerLabel(name: string): string {
  return PROVIDERS[name] ?? name;
}
