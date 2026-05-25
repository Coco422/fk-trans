# Development

fk-trans is a Tauri v2 desktop translator with three local trigger paths:

- Middle-click translation.
- Deliberate left-button text selection translation, controlled by the Selection Trigger setting.
- macOS OCR region translation with `Cmd+Shift+O`, using Screen Recording plus Apple Vision.

## Local Run

```sh
npm install
npm run tauri dev
```

Useful checks before hand testing:

```sh
npm run test:tauri-args
npm run build -- --mode development
cd src-tauri && cargo check --offline && cargo test --offline
```

## macOS Dev Permissions

`npm run tauri dev` runs the bare debug executable:

```text
src-tauri/target/debug/fk-trans
```

macOS TCC treats that executable separately from a release `.app`, so granting a bundled app does not grant the dev binary.

For text capture, grant Accessibility to the current dev executable. For OCR screenshot capture, grant Screen & System Audio Recording to the current dev executable. In Settings > Diagnostics, use the macOS Dev Permissions card to reveal the exact executable path.

If the System Settings picker does not show `fk-trans`, click `+`, press `Cmd+Shift+G`, paste the executable path, then add it.

When launching `npm run tauri dev` from another app, macOS may attribute Screen Recording responsibility to the host process too. For example, a dev run launched from Codex Desktop may require Screen Recording for both `Codex` and `fk-trans`; a run launched from Terminal or iTerm may require the terminal app as well.

After changing Accessibility or Screen Recording permissions, fully stop and restart the dev run:

```sh
pkill -f 'target/debug/fk-trans'
pkill -f 'tauri dev'
pkill -f 'vite'
npm run tauri dev
```

If duplicate `fk-trans` entries appear in Privacy & Security, remove stale entries, rebuild once, then re-add the current `src-tauri/target/debug/fk-trans` executable.

