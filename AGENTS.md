# fk-trans Agent Guide

macOS-only middle-click and select-to-translate tool (Tauri v2 + Solid + Rust).
macOS is the only supported target; there is no cross-platform abstraction to keep
in sync, so prefer AppKit-native behaviour over portable-looking indirection.

## Development Rules

- Follow TDD for app changes: add or update the smallest relevant regression check before or with the fix, then run it.
- Follow KISS: prefer the smallest change that fixes the observed behavior.
- Keep translation triggers observable. Whenever a trigger cannot fire, the
  diagnostics snapshot and report must name the layer that failed — a silent
  no-op is the worst outcome for a tool whose whole job is "middle-click and get
  a translation".
- Tauri command arguments must be verified across the Rust/frontend boundary.
  Frontend calls use Tauri camelCase keys such as `baseUrl`, `apiKey`,
  `systemPrompt`, `userPrompt`, and `extraParams`. Payload *fields* are being
  unified to camelCase too; do not add `source_lang`-style aliases or
  `?? source_lang` fallbacks on the frontend.
- Anything that can hang must have a deadline. `reqwest` has no default timeout,
  and the translation pipeline holds a single-flight gate — an unbounded request
  used to wedge the app permanently. Use `translate::http::shared_client()` rather
  than `reqwest::Client::new()` so every provider gets the shared budget and pool.
- Never touch the user's pasteboard losslessly-partially. Capture goes through
  `clipboard::pasteboard` so the whole pasteboard is backed up and restored (see
  the module docs for why text-only backup destroyed copied images and files).

## Architecture Notes

- The trigger decision is a pure state machine in `trigger::reducer`. It takes
  plain `PointerEvent`s and a clock as a parameter and returns what the event
  meant; `mouse::listener` only converts tap events into that shape and applies
  the answer. Never put a threshold or an ordering rule back into the CGEventTap
  callback — it runs on a thread macOS kills if it is slow, and nothing in it can
  be tested without a live session.
- The reducer does not know whether the panel is on screen, and must not learn.
  It reports *that* a click or a scroll happened; `trigger::panel` decides whether
  that applies. Two things depend on this: the tap sees a click on the trigger
  point before the panel's own handler does, and one place has to know what is
  displayed.
- `trigger::panel` is the panel's state machine as pure data — point or card,
  where each goes, and the generation that stops a slow translation resurrecting
  a panel the user has closed. `macos::panel` applies the `Transition` it
  returns; nothing else touches the window.
- `capture` owns the strategy chain: ask the Accessibility API first, fall back
  to a synthesised `⌘C` only where that is right (`should_fall_back_to_pasteboard`
  is an exhaustive match, so a new failure mode will not compile until it is
  classified). The selection trigger passes `pasteboard_allowed = false` — it only
  needs to know whether to offer a trigger point, and synthesising a copy for
  that would take over the user's clipboard before they asked for anything.
- `macos::panel` swizzles the popup window into a real `NSPanel`. This is not
  cosmetic: a plain `NSWindow` activates the app when clicked (destroying the
  selection being read) and has its first click swallowed by AppKit. If the
  conversion fails the app still runs on a plain window, with a warning.
- `pipeline::PipelineGate` is the single-flight gate for translation runs. It
  records when the current run started and lets a later trigger reclaim the gate
  after `DEFAULT_GATE_LIMIT`, so a wedged run cannot disable the app for good.
  Gate ownership uses a token; a stale run's release must not clear a newer run's
  gate.
- `clipboard::pasteboard::Pasteboard` is the seam for pasteboard access. Tests use
  an in-memory fake; the macOS backend serialises access behind a process-wide
  lock because AppKit's pasteboard segfaults under concurrent access.
- `translate::http` owns the shared HTTP client, its timeouts, the status-to-error
  mapping, and response parsing helpers. Every provider goes through `send()`, so
  no provider can echo an upstream body into a user-visible error.
- `translate::prompt` is the only prompt formatter, and `translate::language`
  is the only place a language code becomes a name or a BCP-47 code. Prompts ask
  for the form they need (`{to}`, `{to_name}`, `{to_code}`) instead of each
  provider mapping codes itself.
- Providers are `openai` (any OpenAI-compatible endpoint, including Ollama),
  `claude`, `gemini`, `deeplx`. Ollama is a *preset* of `openai`, not an
  implementation: its `/v1/chat/completions` is compatible, and `reasoning_effort`
  controls thinking. See `docs/local-models.md`.
- An unknown provider name is an error. It used to fall through to
  `api.openai.com`, silently sending the user's text to a service they had not
  configured.
- `config` writes atomically via a sibling temp file + rename, at mode 0600
  (the file holds API keys in plain text). A config that fails to parse is moved
  aside, reported through `AppState::config_load_problem`, and shown in
  Diagnostics — never silently replaced with defaults. Changes made while loading
  (a removed provider, a rewritten URL) go through `ConfigLoad::notices` and are
  shown too, because a migration the user cannot see is indistinguishable from
  the app editing their settings.

- The popup's extra actions (explain / summary / polish / dict) are
  *instructions to a model*, not translations, and go through
  `TranslateProvider::complete`. Sending one through `translate` wrapped it in
  the translation template, so the model was asked to translate its own
  instruction. `complete` sends no system prompt for the same reason: a
  provider's own is a translation instruction.
- Each action has its own prompt and an optional provider override, in
  `AppConfig::actions`. `validate_actions` refuses a list that would leave the
  popup with a button it cannot honour — a prompt without `{text}` never sees the
  selection, and a missing action takes a button away silently.

## Changing a Provider

Anything request-shaped a preset needs goes in that entry's `extra_params` — it
is merged into the request body. Do not bake a provider-specific flag into the
base body: it is sent to every endpoint, including ones that have never heard of
it. `chat_template_kwargs.enable_thinking = false`, for example, is a vLLM flag
and belongs in the vLLM entry's `extra_params`.

A response that parses but contains no translation is an error, never
`.unwrap_or("")`. An empty success renders as a blank popup with nothing to act
on.

## Frontend

- `src/api/types.ts` is the only hand-written-looking file that is not: it is
  generated. Everything else is `src/store.ts` (shared state for the settings
  window), `src/tabs/` (the four pages) and `src/ui/` (primitives).
- Colour is declared once per theme in `src/styles/globals.css` and exposed to
  Tailwind through `@theme inline`. Components use `bg-surface`, `text-muted`,
  `border-line` and friends — there should be no `dark:` variant and no
  `gray-500` in a component, because the theme is one class on `<html>` and the
  variables underneath it.
- The popup window and the settings window share an origin, so they share the
  stored theme; the popup has no toggle of its own and follows whatever the
  other one chose.
- Biome owns formatting and linting. `src/api/types.ts` is excluded, so the
  formatter cannot rewrite what ts-rs emits.

## Useful Checks

```sh
npm run check                      # Biome + tsc + vite build
cd src-tauri && cargo check --offline && cargo test --offline
```

Keep `--offline` working: pin new dependencies with `cargo fetch` while online,
commit `Cargo.lock`, and do not relax the check to a networked build.

Some tests touch the real system pasteboard (to verify the AppKit bridge, which a
fake cannot). They restore the user's clipboard on every path, including panics,
and serialise against each other.

## Generated TypeScript

`src/api/types.ts` is generated by ts-rs from the Rust structs and **is committed**.
Anything that crosses the Tauri boundary derives `TS` and declares
`#[ts(export, export_to = "types.ts")]`; nothing on that boundary is hand-written.

```sh
npm run gen:types   # after changing any DTO, then commit src/api/types.ts
```

CI regenerates the file and fails if the result differs from what is committed,
so a Rust DTO change without its TypeScript is caught before review.

Two rules that come out of this:

- The wire format is camelCase everywhere. Rust field names stay snake_case and
  are mapped with `#[serde(rename_all = "camelCase")]`.
- Do not add `#[serde(alias)]` to a DTO. It would make the struct itself accept
  the old spelling, including on the IPC path, and ts-rs cannot parse it. Reading
  files written by older builds goes through `compat::from_str_compat` instead —
  one call, at the point where such a file is read.
- `i64` and `u64` fields need `#[ts(type = "number")]`. ts-rs defaults to
  `bigint`, which the frontend never receives and cannot compare against.

