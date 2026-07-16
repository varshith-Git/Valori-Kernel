# Phase D1.3 — Installers + clean-machine validation groundwork

## Goal

Produce a real, installable Valori Desktop package: bundle `ui/` (which
can't statically export yet) as a Node sidecar alongside the existing
`valori-daemon`/`valori-node` sidecars, wire up a macOS `.dmg` build
end-to-end, and scaffold a cross-platform CI matrix (macOS/Windows/Linux) so
Windows `.msi` and Linux `.AppImage` get produced even though only macOS
could be built and verified locally in this environment.

## Delivered

- **Fixed a real, pre-existing build blocker**: two API route handlers
  (`src/app/api/records/[id]/route.ts`, `.../metadata/route.ts`) still used
  Next.js 14's synchronous `{ params: { id: string } }` signature; this repo's
  Next.js version requires `{ params: Promise<{ id: string }> }` + `await
  params`. This blocked `next build` outright (not just `tsc --noEmit`,
  which had surfaced the same error earlier but wasn't treated as blocking at
  the time). Fixed both to match the convention already used elsewhere in the
  codebase (e.g. `api/projects/[name]/route.ts`).
- **`ui/` bundled as a Node sidecar** (the "Option A" call from earlier
  discussion — keep Next.js, don't migrate to static export yet):
  - `desktop/scripts/prepare-ui-server.mjs` (new) — runs `next build`
    (`output: "standalone"`, already configured), copies `.next/static` into
    the standalone tree (Next deliberately excludes it — documented
    behavior), copies the result into `src-tauri/resources/ui-server/` as a
    Tauri bundle *resource* (not a sidecar — it's a JS app + `node_modules`,
    not one self-contained executable).
  - `desktop/scripts/prepare-sidecars.mjs` — extended to also copy the Node
    runtime currently running the script (`process.execPath`) as a genuine
    `node` sidecar (one self-contained executable, unlike `ui-server/`), and
    to ensure a placeholder `resources/ui-server/` directory exists in dev
    mode (Tauri's build script validates `resources` paths on every cargo
    build too, not just `externalBin` — same class of issue found in D3.1).
  - `desktop/src-tauri/src/ui_server_manager.rs` (new) — release-mode only:
    spawns the `node` sidecar against the bundled `ui-server/server.js`
    resource (fixed port `17862`), polls until healthy, then calls
    `WebviewWindow::navigate()` to switch the main window from a small
    "Starting Valori…" loading page (`src-tauri/loading/`, the new
    `frontendDist`) to the real app. Dev mode is completely unaffected —
    `tauri dev` still runs `ui/`'s own `next dev` via `devUrl`.
  - `tauri.conf.json` — `externalBin` gains `binaries/node`;
    `bundle.resources` maps `resources/ui-server/` → `ui-server/`;
    `beforeBuildCommand` chains both prep scripts.
- **Real end-to-end `tauri build` achieved for the first time in this
  project** — produced and verified `Valori.app` + a checksum-verified
  `Valori_0.1.0_aarch64.dmg` (44 MB). Confirmed via direct launch (not just
  build success): the `node` sidecar starts, binds `17862`, serves the real
  Next.js app (`GET /` → 200); `valori-daemon`/`valori-node`/`node` sidecars
  and the `ui-server` resource all sit correctly inside
  `Contents/MacOS/`/`Contents/Resources/` as documented in
  [`docs/architecture/desktop-layout.md`](../architecture/desktop-layout.md)
  (new).
- **Found and fixed a real shutdown bug via testing, not by inspection**:
  sending a raw SIGTERM directly to the packaged app (simulating a session
  logout / `killall` / force-quit, not a normal window-close) left the
  bundled `node`/`ui-server` child process running and the port held — the
  graceful `RunEvent::ExitRequested` handler only fires for Tauri-initiated
  exits, not external signals. Fixed with a `#[cfg(unix)]` SIGTERM handler
  that runs the same cleanup path. Fixing this also surfaced a **second**,
  more subtle bug in the same code before it ever shipped: the existing
  `ExitRequested` handler called `AppHandle::exit()` at the end of its own
  async cleanup, which (per Tauri's own docs) re-triggers `ExitRequested` —
  re-entering the same handler, calling `prevent_exit()` again, and
  re-spawning cleanup, a real infinite-loop-on-quit risk that had never been
  exercised (the only shutdown path tested before this phase was the direct
  `stop_daemon_internal()` unit test, and a SIGTERM test that bypassed this
  handler entirely). Fixed both with a shared `Arc<AtomicBool>` guard: the
  real shutdown work runs once; a subsequent `ExitRequested` (from our own
  final `std::process::exit(0)` instead of `AppHandle::exit()`) is a no-op
  that lets the process actually exit.
- **`.github/workflows/desktop-build.yml`** (new) — matrix build
  (macos-14/windows-latest/ubuntu-22.04), each running the real
  `desktop && npm run build` pipeline and uploading the platform's installer
  as a workflow artifact. Linux job installs `libwebkit2gtk-4.1-dev` +
  AppImage build deps. Code signing/notarization explicitly out of scope
  (Phase D1.4).
- **[`docs/architecture/desktop-layout.md`](../architecture/desktop-layout.md)**
  (new) — the real bundle layout (which binaries are sidecars vs.
  resources, and why), the startup sequence, the workspace directory layout,
  and the fixed loopback ports.
- **[`docs/DESKTOP_RELEASE_CHECKLIST.md`](../DESKTOP_RELEASE_CHECKLIST.md)**
  (new) — manual clean-machine smoke test steps (install → launch → Welcome
  → create project/collection → insert/search → quit → relaunch → verify
  persistence), deliberately not automated per explicit scope for this phase.

## Findings

- Tauri's build script validates **both** `externalBin` and `bundle.resources`
  paths on every cargo build, dev included — not just `tauri build`. Same
  class of issue as D3.1's `externalBin` discovery; `dev.mjs`/
  `prepare-sidecars.mjs` now also ensure a placeholder `resources/ui-server/`
  directory exists in dev mode.
- `bundle_dmg.sh` (the `create-dmg`-based DMG bundler Tauri invokes) fails if
  a previous build's read-write DMG image is still mounted — each failed
  attempt leaves a mounted `/Volumes/dmg.XXXX` behind, and the *next*
  attempt's failure looked like a fresh, unrelated error until the mounts
  were traced back with `hdiutil info`. Not a code bug, but worth knowing:
  `hdiutil detach` any stale `dmg.*` volumes before retrying a failed
  `tauri build`.
- `next build`'s standalone output does not include `.next/static/` — this
  is documented Next.js behavior (assumed to be served from a CDN in a real
  deployment), not a bug, but it means naively bundling `.next/standalone/`
  as-is produces a server that 404s on every JS/CSS asset. Handled by
  `prepare-ui-server.mjs`'s explicit copy step.

## Validation

- `cargo test` in `desktop/src-tauri` — still 2 passed (unchanged from D3.1;
  the sidecar-spawn code paths remain covered by build-time type-checking,
  not a runtime test — see Follow-ups).
- Real `tauri build` (`cd desktop && npm run build`) — succeeded, produced
  `Valori.app` and a `hdiutil verify`-clean `Valori_0.1.0_aarch64.dmg`.
- Real launch of the packaged `.app` (not the dev binary): confirmed via
  `lsof` that the bundled `node` sidecar binds `127.0.0.1:17862`; confirmed
  via `curl` that `GET http://127.0.0.1:17862/` returns `200` (the real
  Next.js app, not the loading page); confirmed via `ps`/`lsof` after sending
  SIGTERM to the parent that both the daemon path (already covered by D3.1's
  automated test) and the newly-added ui-server cleanup leave no orphaned
  process and no held port.
- **Not done**: the full manual click-through checklist in
  `DESKTOP_RELEASE_CHECKLIST.md` (Welcome → create project → collection →
  insert → search → quit → relaunch → verify persistence) requires actual
  GUI interaction with the webview, which this environment has no tooling
  for (no click automation, `screencapture` failed — no display access).
  Verified the pieces that *can* be checked headlessly/via HTTP instead
  (above); the click-through itself is the single most valuable next
  verification step and needs either a human or a future session with
  screen-automation/VM access.

## Follow-ups

- Run `docs/DESKTOP_RELEASE_CHECKLIST.md` for real, on all three platforms —
  the one thing this phase could not verify itself.
- Windows `.msi` / Linux `.AppImage` have never actually been built —
  `desktop-build.yml` is written correctly against documented Tauri/Next
  behavior but is unverified on those platforms. Watch the first CI run.
- The `RunningChild::Sidecar` spawn path (daemon/node) and the
  `ui_server_manager` sidecar spawn path are both still only exercised by
  real end-to-end launches done manually in this session, not by an
  automated test — same gap flagged in D3.1, now with two spawn sites
  instead of one.
- D1.4 (code signing + notarization) is next per the user's own stated
  sequence — only relevant once ready to distribute the app publicly.
- D2 (auto-updater via GitHub Releases) comes after D1.4.
