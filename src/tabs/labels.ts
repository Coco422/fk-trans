/**
 * What things are called on screen.
 *
 * The *sets* come from Rust — which languages a prompt can be built for, which
 * providers exist, which actions the popup offers — because those are the parts
 * the backend has to agree with. What each is called in this window is this
 * window's business, so the names are here. Anything without an entry falls back
 * to its id rather than disappearing.
 */

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
  openai: "OpenAI 兼容",
  ollama: "Ollama（本地）",
};

export function providerLabel(name: string): string {
  return PROVIDERS[name] ?? name;
}

/** The popup's extra actions, by id. */
const ACTIONS: Record<string, string> = {
  explain: "解释",
  summary: "总结",
  polish: "润色",
  dict: "词典",
};

export function actionLabel(id: string): string {
  return ACTIONS[id] ?? id;
}
