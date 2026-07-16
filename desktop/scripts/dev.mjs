#!/usr/bin/env node
// `beforeDevCommand` entry point: prepares the sidecar binaries synchronously
// (required for every desktop cargo build, dev included — see
// prepare-sidecars.mjs), then starts the `ui/` dev server as the long-running
// foreground process `tauri dev` expects.

import { spawn } from "node:child_process";
import { prepareSidecars } from "./prepare-sidecars.mjs";

prepareSidecars({ release: false });

const child = spawn("npm", ["--prefix", "../ui", "run", "dev"], {
  stdio: "inherit",
  shell: true,
});
child.on("exit", (code) => process.exit(code ?? 0));
