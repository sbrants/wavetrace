#!/usr/bin/env node
/**
 * Assemble latest.json for the Tauri updater from GitHub Release assets.
 * Usage: node scripts/publish-latest-json.mjs <assets-dir>
 */
import fs from "fs";
import path from "path";
import { fileURLToPath } from "url";

const assetsDir = process.argv[2];
if (!assetsDir) {
  console.error("Usage: node scripts/publish-latest-json.mjs <assets-dir>");
  process.exit(1);
}

const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const pkg = JSON.parse(fs.readFileSync(path.join(repoRoot, "package.json"), "utf8"));
const version = process.env.VERSION || pkg.version;
const tag = process.env.RELEASE_TAG || `v${version}`;
const repo = process.env.GITHUB_REPOSITORY || "sbrants/wavetrace";

const files = fs.readdirSync(assetsDir);

function readSig(bundleName) {
  const sigPath = path.join(assetsDir, `${bundleName}.sig`);
  if (!fs.existsSync(sigPath)) {
    return null;
  }
  return fs.readFileSync(sigPath, "utf8").trim();
}

function releaseUrl(name) {
  return `https://github.com/${repo}/releases/download/${tag}/${name}`;
}

const platforms = {};

const winNsis = files.find((f) => f.endsWith(".nsis.zip"));
const winExe = files.find((f) => f.includes("-setup") && f.endsWith(".exe"));
const winBundle = winNsis || winExe;
if (winBundle) {
  const signature = readSig(winBundle);
  if (signature) {
    platforms["windows-x86_64"] = { url: releaseUrl(winBundle), signature };
  }
}

const appImage = files.find((f) => f.endsWith(".AppImage"));
if (appImage) {
  const signature = readSig(appImage);
  if (signature) {
    platforms["linux-x86_64"] = { url: releaseUrl(appImage), signature };
  }
}

for (const arch of ["aarch64", "x86_64"]) {
  const bundle = files.find(
    (f) => f.includes(`_macos_${arch}.app.tar.gz`) && !f.endsWith(".sig")
  );
  if (!bundle) {
    continue;
  }
  const signature = readSig(bundle);
  if (signature) {
    platforms[`darwin-${arch}`] = { url: releaseUrl(bundle), signature };
  }
}

const required = [
  "windows-x86_64",
  "linux-x86_64",
  "darwin-aarch64",
  "darwin-x86_64",
];
const missing = required.filter((key) => !platforms[key]);
if (missing.length > 0) {
  console.error(`Missing updater assets for: ${missing.join(", ")}`);
  console.error(`Files in ${assetsDir}: ${files.join(", ")}`);
  process.exit(1);
}

const manifest = {
  version,
  notes: process.env.RELEASE_NOTES || "",
  pub_date: new Date().toISOString(),
  platforms,
};

process.stdout.write(`${JSON.stringify(manifest, null, 2)}\n`);
