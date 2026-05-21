#!/usr/bin/env bash
set -euo pipefail

target="${TAURI_TARGET:-aarch64-apple-darwin}"

if [[ -z "${TAURI_SIGNING_PRIVATE_KEY:-}" ]]; then
  key_path="${TAURI_SIGNING_PRIVATE_KEY_PATH:-}"

  if [[ -z "$key_path" ]]; then
    if [[ -f "$HOME/.tauri/fk-trans.key" ]]; then
      key_path="$HOME/.tauri/fk-trans.key"
    elif [[ -f "/Users/ray/.tauri/fk-trans.key" ]]; then
      key_path="/Users/ray/.tauri/fk-trans.key"
    else
      echo "Missing updater signing key. Set TAURI_SIGNING_PRIVATE_KEY or TAURI_SIGNING_PRIVATE_KEY_PATH." >&2
      exit 1
    fi
  fi

  export TAURI_SIGNING_PRIVATE_KEY="$(<"$key_path")"
fi

export TAURI_SIGNING_PRIVATE_KEY_PASSWORD="${TAURI_SIGNING_PRIVATE_KEY_PASSWORD:-}"

tauri build --target "$target"
TAURI_TARGET="$target" node scripts/write-latest-json.mjs
