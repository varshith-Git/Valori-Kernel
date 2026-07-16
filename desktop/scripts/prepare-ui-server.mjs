#!/usr/bin/env node
// Release-only: packages `ui/` (Next.js, `output: "standalone"`) as a bundled
// Tauri resource — so a real install never needs Node.js installed on the
// end user's machine (the `node` sidecar that executes it is prepared by
// `prepare-sidecars.mjs`, which runs first in `beforeBuildCommand`).
//
// Dev mode doesn't use any of this — `tauri dev` still runs `ui/`'s own
// `next dev` server directly via `beforeDevCommand` (`dev.mjs`), unchanged.
//
// Steps:
//   1. `next build` in ui/ (standalone output: self-contained server.js +
//      traced node_modules).
//   2. Copy `.next/static` into the standalone tree — Next's standalone
//      output deliberately excludes it (documented Next.js behavior; it's
//      normally served from a CDN in a real deployment).
//   3. Copy the whole standalone tree into
//      `src-tauri/resources/ui-server/` (a Tauri bundle resource, not an
//      externalBin — it's a JS app + node_modules, not one self-contained
//      executable).

import { execFileSync } from "node:child_process";
import { cpSync, existsSync, mkdirSync, rmSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const __dirname = dirname(fileURLToPath(import.meta.url));
const uiDir = join(__dirname, "..", "..", "ui");
const srcTauriDir = join(__dirname, "..", "src-tauri");
const resourcesDir = join(srcTauriDir, "resources", "ui-server");
const isWindows = process.platform === "win32";

console.log("[prepare-ui-server] next build (standalone)");
execFileSync("npm", ["run", "build"], { cwd: uiDir, stdio: "inherit", shell: isWindows });

const standaloneDir = join(uiDir, ".next", "standalone");
if (!existsSync(standaloneDir)) {
  throw new Error(`${standaloneDir} missing — is next.config.ts still set to output: "standalone"?`);
}

console.log("[prepare-ui-server] copying .next/static into standalone tree");
cpSync(join(uiDir, ".next", "static"), join(standaloneDir, ".next", "static"), { recursive: true });

// Next's standalone output excludes `public/` too — same reasoning as
// `.next/static` above (normally CDN-served). Without this, static assets
// (logo, favicons, etc.) 404 in the packaged app since server.js is run
// directly with no separate static layer in front of it.
const publicDir = join(uiDir, "public");
if (existsSync(publicDir)) {
  console.log("[prepare-ui-server] copying public/ into standalone tree");
  cpSync(publicDir, join(standaloneDir, "public"), { recursive: true });
}

console.log(`[prepare-ui-server] copying standalone tree -> ${resourcesDir}`);
rmSync(resourcesDir, { recursive: true, force: true });
mkdirSync(join(srcTauriDir, "resources"), { recursive: true });
cpSync(standaloneDir, resourcesDir, { recursive: true });
