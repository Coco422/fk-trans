# fk-trans Agent Guide

## Development Rules

- Follow TDD for app changes: add or update the smallest relevant regression check before or with the fix, then run it.
- Follow KISS: prefer the smallest change that fixes the observed behavior; avoid broad refactors unless the failing path requires them.
- Tauri command arguments must be verified across the Rust/frontend boundary. Frontend calls use Tauri camelCase keys such as `baseUrl`, `apiKey`, `systemPrompt`, `userPrompt`, and `extraParams`.
- Keep middle-click translation observable. If it cannot trigger, expose the failed layer in Diagnostics instead of silently doing nothing.
- For macOS dev permission bugs, remember `npm run tauri dev` runs `src-tauri/target/debug/fk-trans`, not the release `.app`; Screen Recording may also need the host app such as Codex, Terminal, or iTerm. Keep details aligned with `docs/development.md`.

## Useful Checks

```sh
npm run test:tauri-args
npm run build -- --mode development
cd src-tauri && cargo check --offline && cargo test --offline
```
