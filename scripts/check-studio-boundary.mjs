#!/usr/bin/env node
// Shared Studio anti-duplication guard (Phase I).
//
// Invariant: every canonical module listed in studio-boundary.json has
// exactly ONE source file, inside @valori/studio. This script recursively
// scans a host application's src/ tree and fails if any of those exact
// filenames exist outside the (optional) --studio directory.
//
// Deliberately dumb, on purpose: filename matching only, no import-graph
// analysis, no AST parsing. A host wrapper (CloudToolsWorkspace.tsx,
// LocalFilesPanel.tsx, ReceiptCard.tsx, ...) has a different filename than
// the canonical module it wraps, so it never matches — no false positives
// from that direction. And because this matches whole filenames, not
// substrings, "NodeDetail.tsx" or "GraphViewerHelper.tsx" never collide
// with "NodeCard.tsx" / "GraphView.tsx" either.
//
// Usage:
//   node scripts/check-studio-boundary.mjs <hostSrcDir> [--studio <studioSrcDir>]
//
// Examples:
//   node scripts/check-studio-boundary.mjs ui/src --studio ui/studio/src   (Valori-Kernel)
//   node scripts/check-studio-boundary.mjs ui/src                         (valori-ui — no local Studio copy to exclude)

import { readFileSync, readdirSync, statSync } from "node:fs";
import { join, dirname, resolve, relative } from "node:path";
import { fileURLToPath } from "node:url";

const __dirname = dirname(fileURLToPath(import.meta.url));

function parseArgs(argv) {
  const args = argv.slice(2);
  const positional = [];
  let studioDir = null;
  for (let i = 0; i < args.length; i++) {
    if (args[i] === "--studio") {
      studioDir = args[++i];
    } else {
      positional.push(args[i]);
    }
  }
  if (positional.length !== 1) {
    console.error("Usage: node check-studio-boundary.mjs <hostSrcDir> [--studio <studioSrcDir>]");
    process.exit(2);
  }
  return { hostSrcDir: positional[0], studioDir };
}

function loadCanonicalModules() {
  const listPath = join(__dirname, "studio-boundary.json");
  const data = JSON.parse(readFileSync(listPath, "utf8"));
  return new Set(data.modules);
}

// Ignore build output / dependency dirs a src/ walk should never need to
// enter — keeps this fast and avoids false hits inside compiled/vendored code.
const SKIP_DIRS = new Set(["node_modules", ".next", "dist", ".git"]);

function walk(dir, onFile) {
  let entries;
  try {
    entries = readdirSync(dir, { withFileTypes: true });
  } catch {
    return;
  }
  for (const entry of entries) {
    if (entry.isDirectory()) {
      if (SKIP_DIRS.has(entry.name)) continue;
      walk(join(dir, entry.name), onFile);
    } else if (entry.isFile()) {
      onFile(join(dir, entry.name));
    }
  }
}

function main() {
  const { hostSrcDir, studioDir } = parseArgs(process.argv);
  const canonical = loadCanonicalModules();

  const hostAbs = resolve(hostSrcDir);
  const studioAbs = studioDir ? resolve(studioDir) : null;

  if (statSync(hostAbs, { throwIfNoEntry: false }) === undefined) {
    console.error(`error: host src dir not found: ${hostSrcDir}`);
    process.exit(2);
  }

  const violations = [];

  walk(hostAbs, (filePath) => {
    // Never flag a match inside the canonical Studio tree itself — only
    // relevant for Valori-Kernel, where studio/ lives inside the same repo
    // (and, depending on the caller's arguments, potentially inside the
    // scanned dir's ancestry).
    if (studioAbs && filePath.startsWith(studioAbs)) return;

    const base = filePath.split("/").pop();
    if (canonical.has(base)) {
      violations.push(filePath);
    }
  });

  if (violations.length > 0) {
    console.error("Shared Studio duplication detected:\n");
    for (const v of violations) {
      const base = v.split("/").pop();
      console.error(`  ${base}`);
      console.error(`    canonical: @valori/studio (ui/studio/src/**/${base})`);
      console.error(`    duplicate: ${relative(process.cwd(), v)}`);
      console.error("");
    }
    console.error(
      "This filename is reserved for the single shared implementation in @valori/studio.\n" +
      "If this is meant to extend or wrap the shared feature, give it a distinct name\n" +
      "(e.g. CloudToolsWorkspace.tsx, LocalFilesPanel.tsx) and import the canonical\n" +
      "component from '@valori/studio' instead of reimplementing it.\n" +
      "See docs/architecture/shared-studio.md."
    );
    process.exit(1);
  }

  console.log(`Shared Studio boundary check passed (${hostSrcDir}): no duplicate canonical modules found.`);
}

main();
