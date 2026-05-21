# Release

fk-trans is configured for macOS Apple Silicon releases and Tauri updater artifacts.

## Local macOS build

The updater signing key was generated at:

```text
~/.tauri/fk-trans.key
```

Build a local arm64 release:

```sh
npm run release:mac
```

To use a different local key file:

```sh
TAURI_SIGNING_PRIVATE_KEY_PATH=/path/to/key npm run release:mac
```

The DMG is written to:

```text
src-tauri/target/aarch64-apple-darwin/release/bundle/dmg/
```

The updater package, signature, and `latest.json` are written under the matching `bundle/macos/` output.

For manual GitHub Release uploads, publish these files to the matching tag release:

```text
src-tauri/target/aarch64-apple-darwin/release/bundle/dmg/fk-trans_{version}_aarch64.dmg
src-tauri/target/aarch64-apple-darwin/release/bundle/macos/fk-trans.app.tar.gz
src-tauri/target/aarch64-apple-darwin/release/bundle/macos/fk-trans.app.tar.gz.sig
src-tauri/target/aarch64-apple-darwin/release/bundle/macos/latest.json
```

By default `latest.json` points to `app-v{version}`. Override that when building a manual release with:

```sh
RELEASE_TAG=app-v0.1.4 npm run release:mac
```

## GitHub Actions release

Add this repository secret before publishing from GitHub Actions:

```text
TAURI_SIGNING_PRIVATE_KEY
```

Use the contents of `~/.tauri/fk-trans.key` as the secret value. The current key has no password, so `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` can stay unset.

To publish a release:

1. Bump the version in `package.json`, `src-tauri/Cargo.toml`, and `src-tauri/tauri.conf.json`.
2. Commit and push the version bump.
3. Create and push a tag such as `app-v0.1.4`.

```sh
git tag app-v0.1.4
git push origin app-v0.1.4
```

The workflow uploads the DMG plus Tauri updater artifacts to the GitHub Release. The app checks:

```text
https://github.com/Coco422/fk-trans/releases/latest/download/latest.json
```

## macOS signing

The current release workflow signs Tauri updater artifacts, but it does not use Apple Developer ID signing or notarization. That is fine for personal use, though macOS Gatekeeper may require manually allowing the app after download.

For smoother distribution later, add Apple Developer ID certificate secrets and notarization settings to `.github/workflows/release.yml`.
