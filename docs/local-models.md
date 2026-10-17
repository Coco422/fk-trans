# Local models (Ollama)

Ollama is reached over its **OpenAI-compatible endpoint**, so it is a preset of
the `openai` provider rather than an implementation of its own. Nothing is
bundled: the app is an HTTP client for a server you run.

## Recommended model

```sh
ollama pull translategemma:4b      # 3.3 GB, 128K context, 55 languages
```

`translategemma` is a translation-specialised family built on Gemma 3, and 4b is
its smallest size. It is a better default than a general small model because the
whole job here is translation.

`qwen3:4b` (~2.5 GB) is a reasonable general-purpose alternative, but it is a
*thinking* model — see below before using it.

The shipped default config points the `ollama` entry at

```
baseUrl: http://127.0.0.1:11434/v1
model:   translategemma:4b
extraParams: { "reasoning_effort": "none" }
```

## Thinking models

A thinking model puts its reasoning in a `reasoning` field and leaves `content`
empty until it finishes. With a small token budget it can spend the entire budget
on reasoning and return no translation at all. Measured against Ollama, asking
`gemma4:12b-it-qat` for a one-word reply:

| `reasoning_effort` | `content` | `reasoning` | `finish_reason` |
| --- | --- | --- | --- |
| absent | `""` | 124 chars | `stop` |
| `"none"` | `"OK"` | absent | `stop` |

So `reasoning_effort: "none"` in `extraParams` is required, not cosmetic. Ollama
maps it to its native `think: false`, and it is harmless for a model that does
not think. Accepted values: `minimal`, `low`, `medium`, `high`, `xhigh`, `ultra`,
`max`, `none`.

If a thinking model is configured without it, the app reports:

> The model replied with reasoning (N chars) but no translation: it is in thinking
> mode. Turn it off in this provider's Extra Parameters — for Ollama,
> `{"reasoning_effort": "none"}`.

## Prompt format

TranslateGemma documents a prompt that names the language *and* its BCP-47 code:

```
You are a professional {source name} ({source code}) to {target name} ({target code}) translator. …
```

The app renders those from `translate::language`, so `{from}` and `{to}` produce
`English (en)` and `Chinese (zh-Hans)`. Note that Simplified Chinese is
`zh-Hans` in that table, not `zh`. `{from_name}` / `{from_code}` / `{to_name}` /
`{to_code}` are available when a template needs one form on its own.

## Other endpoints

Any OpenAI-compatible server works — point the `openai` entry's base URL at it
and set the model. A loopback base URL needs no API key; anything else does.
