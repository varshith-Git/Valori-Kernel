#!/usr/bin/env node
// Prepares the Tauri sidecar binaries (`valori-daemon`, `valori-node`) that
// get bundled into the desktop app (Phase D3.1 — no user ever needs a
// separate binary or a terminal). Tauri's `externalBin` config
// (`src-tauri/tauri.conf.json`) requires files named
// `<name>-<target-triple>[.exe]` to exist under `src-tauri/binaries/` before
// *any* cargo build of the desktop crate — including `tauri dev` — so this
// runs from both `beforeDevCommand` (via `dev.mjs`) and `beforeBuildCommand`.
//
// Dev builds reuse whatever's already compiled (release preferred, else
// debug) and only build (debug, fast) if nothing exists yet — the dev-mode
// code path in `daemon_manager.rs` never actually spawns these sidecar
// files, so their exact freshness doesn't matter, only their presence.
// Release builds (`--release` flag) always rebuild in release mode, since
// those are the binaries a real user's install will run.
//
// Also copies the Node runtime binary into `resources/ValoriUIServer.app/Contents/MacOS/`
// (the helper app bundle that gives the node process LSUIElement=YES via its Info.plist,
// suppressing the macOS Dock icon) and ensures `resources/ui-server/` exists.
// Both have to happen in dev mode too: Tauri's build script validates every `resources`
// path on every cargo build. Dev mode only needs the resource directory to exist,
// not be a real build — `prepare-ui-server.mjs` (release only) fills it in for real.

import { execFileSync } from "node:child_process";
import { copyFileSync, chmodSync, existsSync, mkdirSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const __dirname = dirname(fileURLToPath(import.meta.url));
const repoRoot = join(__dirname, "..", ".."); // desktop/scripts -> desktop -> repo root
const srcTauriDir = join(__dirname, "..", "src-tauri");
const binDir = join(srcTauriDir, "binaries");
const uiServerResourceDir = join(srcTauriDir, "resources", "ui-server");
const isWindows = process.platform === "win32";
const BINARIES = ["valori-daemon", "valori-node"];

function hostTriple() {
  const out = execFileSync("rustc", ["-vV"], { encoding: "utf8" });
  const match = out.match(/^host:\s*(\S+)$/m);
  if (!match) throw new Error("could not determine host target triple from `rustc -vV`");
  return match[1];
}

function exeName(name) {
  return isWindows ? `${name}.exe` : name;
}

function targetPath(profile, name) {
  return join(repoRoot, "target", profile, exeName(name));
}

function cargoBuild(name, release) {
  const args = ["build", "-p", name];
  if (release) args.push("--release");
  console.log(`[prepare-sidecars] cargo ${args.join(" ")}`);
  execFileSync("cargo", args, { cwd: repoRoot, stdio: "inherit" });
}

// Helper app bundle: node runs from here so macOS finds LSUIElement=YES in the
// bundle's Info.plist and suppresses the Dock icon.
const uiServerHelperMacOSDir = join(
  srcTauriDir, "resources", "ValoriUIServer.app", "Contents", "MacOS",
);

/** @param {{ release?: boolean }} opts */
export function prepareSidecars({ release = false } = {}) {
  mkdirSync(binDir, { recursive: true });
  const triple = hostTriple();

  for (const name of BINARIES) {
    let source;
    if (release) {
      if (!existsSync(targetPath("release", name))) cargoBuild(name, true);
      source = targetPath("release", name);
    } else {
      source =
        [targetPath("release", name), targetPath("debug", name)].find(existsSync) ?? null;
      if (!source) {
        cargoBuild(name, false);
        source = targetPath("debug", name);
      }
    }

    const dest = join(binDir, exeName(`${name}-${triple}`));
    copyFileSync(source, dest);
    if (!isWindows) chmodSync(dest, 0o755);
    console.log(`[prepare-sidecars] ${name}: ${source} -> ${dest}`);
  }

  if (!existsSync(uiServerResourceDir)) {
    mkdirSync(uiServerResourceDir, { recursive: true });
    console.log(`[prepare-sidecars] created placeholder ${uiServerResourceDir} (dev mode doesn't use it)`);
  }

  // Copy node into the ValoriUIServer.app helper bundle so that when macOS
  // resolves the executable's bundle, it finds the Info.plist with
  // LSUIElement=YES and suppresses the Dock icon automatically.
  mkdirSync(uiServerHelperMacOSDir, { recursive: true });
  const helperNodeDest = join(uiServerHelperMacOSDir, exeName("node"));
  copyFileSync(process.execPath, helperNodeDest);
  if (!isWindows) chmodSync(helperNodeDest, 0o755);
  console.log(`[prepare-sidecars] helper node: ${process.execPath} -> ${helperNodeDest}`);
}

const isMain = process.argv[1] && fileURLToPath(import.meta.url) === process.argv[1];
if (isMain) {
  prepareSidecars({ release: process.argv.includes("--release") });
}
