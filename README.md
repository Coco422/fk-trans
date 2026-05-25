# fk-trans

Local desktop translation tool built with Tauri, Solid, and TypeScript.

## Features

- Middle-click and deliberate text-selection translation.
- `Cmd+Shift+T` fallback shortcut for selected text.
- macOS OCR screenshot-region translation with `Cmd+Shift+O`.
- Provider configuration, history, diagnostics, and popup translation actions.

## Development

```sh
npm install
npm run tauri dev
```

See [docs/development.md](docs/development.md) for macOS dev permissions, OCR Screen Recording setup, and verification commands.

## Release

macOS Apple Silicon builds and updater artifacts are configured for personal releases.

```sh
npm run release:mac
```

See [docs/release.md](docs/release.md) for GitHub Actions release and updater setup.

## Recommended IDE Setup

- [VS Code](https://code.visualstudio.com/) + [Tauri](https://marketplace.visualstudio.com/items?itemName=tauri-apps.tauri-vscode) + [rust-analyzer](https://marketplace.visualstudio.com/items?itemName=rust-lang.rust-analyzer)
