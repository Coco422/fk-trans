import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";

const root = dirname(dirname(fileURLToPath(import.meta.url)));
const appSource = readFileSync(join(root, "src", "App.tsx"), "utf8");
const argsSource = readFileSync(join(root, "src", "tauriArgs.ts"), "utf8");

assert.match(
  appSource,
  /invoke\("update_provider",\s*buildUpdateProviderArgs\(name,\s*draft\)\)/,
  "update_provider must use the typed Tauri argument builder"
);

for (const key of ["baseUrl", "apiKey", "systemPrompt", "userPrompt", "extraParams"]) {
  assert.match(argsSource, new RegExp(`\\b${key}:`), `missing camelCase key ${key}`);
}

assert.doesNotMatch(
  appSource,
  /invoke\("update_provider",\s*\{[\s\S]*?base_url:/,
  "update_provider cannot pass snake_case keys to Tauri"
);
