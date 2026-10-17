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

- `pipeline::PipelineGate` is the single-flight gate for translation runs. It
  records when the current run started and lets a later trigger reclaim the gate
  after `DEFAULT_GATE_LIMIT`, so a wedged run cannot disable the app for good.
  Gate ownership uses a token; a stale run's release must not clear a newer run's
  gate.
- `clipboard::pasteboard::Pasteboard` is the seam for pasteboard access. Tests use
  an in-memory fake; the macOS backend serialises access behind a process-wide
  lock because AppKit's pasteboard segfaults under concurrent access.
- `translate::http` owns the shared HTTP client and its timeouts.
- `config` writes atomically via a sibling temp file + rename, at mode 0600
  (the file holds API keys in plain text). A config that fails to parse is moved
  aside, reported through `AppState::config_load_problem`, and shown in
  Diagnostics — never silently replaced with defaults.

## Useful Checks

```sh
npm run test:tauri-args
npm run build -- --mode development
cd src-tauri && cargo check --offline && cargo test --offline
```

Keep `--offline` working: pin new dependencies with `cargo fetch` while online,
commit `Cargo.lock`, and do not relax the check to a networked build.

Some tests touch the real system pasteboard (to verify the AppKit bridge, which a
fake cannot). They restore the user's clipboard on every path, including panics,
and serialise against each other.
