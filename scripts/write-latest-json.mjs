import { execFileSync } from "node:child_process";
import { existsSync, readFileSync, writeFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const tauriConfig = JSON.parse(
  readFileSync(path.join(root, "src-tauri", "tauri.conf.json"), "utf8")
);

const target = process.env.TAURI_TARGET || "aarch64-apple-darwin";
const version = tauriConfig.version;
const productName = tauriConfig.productName;
const releaseTag = process.env.RELEASE_TAG || process.env.GITHUB_REF_NAME || `app-v${version}`;
const bundleDir = path.join(
  root,
  "src-tauri",
  "target",
  target,
  "release",
  "bundle",
  "macos"
);
const updaterName = `${productName}.app.tar.gz`;
const updaterPath = path.join(bundleDir, updaterName);
const signaturePath = `${updaterPath}.sig`;

if (!existsSync(updaterPath) || !existsSync(signaturePath)) {
  throw new Error(`Missing updater artifact or signature under ${bundleDir}`);
}

function githubRepository() {
  if (process.env.GITHUB_REPOSITORY) {
    return process.env.GITHUB_REPOSITORY;
  }

  try {
    const remote = execFileSync("git", ["remote", "get-url", "origin"], {
      cwd: root,
      encoding: "utf8",
    }).trim();
    const match = remote.match(/github\.com[:/](.+?)(?:\.git)?$/);
    return match?.[1];
  } catch {
    return undefined;
  }
}

function updaterTarget() {
  if (target.startsWith("aarch64-apple-darwin")) {
    return "darwin-aarch64";
  }
  if (target.startsWith("x86_64-apple-darwin")) {
    return "darwin-x86_64";
  }
  throw new Error(`Unsupported updater target: ${target}`);
}

const repo = githubRepository() || "Coco422/fk-trans";
const platform = updaterTarget();
const signature = readFileSync(signaturePath, "utf8").trim();
const url = `https://github.com/${repo}/releases/download/${releaseTag}/${updaterName}`;
const platformEntry = { signature, url };

const latest = {
  version,
  notes: `${productName} ${version}`,
  pub_date: new Date().toISOString(),
  platforms: {
    [`${platform}-app`]: platformEntry,
    [platform]: platformEntry,
  },
};

const latestPath = path.join(bundleDir, "latest.json");
writeFileSync(latestPath, `${JSON.stringify(latest, null, 2)}\n`);
console.log(`Wrote ${latestPath}`);
