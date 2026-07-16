# Phase D3.1 — Bundle the daemon (and node) as Tauri sidecars

## Goal

Close the last "you need Cargo/a terminal" gap identified after D3: bundle
`valori-daemon` and `valori-node` into the desktop app itself via Tauri
sidecars, so a packaged install never needs either binary to exist anywhere
on the user's machine outside the app bundle. Per explicit user direction,
scoped tightly — no status page, no updater, no tray, no event stream.

## Delivered

- `desktop/src-tauri/tauri.conf.json` — `bundle.externalBin: ["binaries/valori-daemon", "binaries/valori-node"]`;
  `build.beforeDevCommand` → `node scripts/dev.mjs`; `build.beforeBuildCommand` → `node scripts/prepare-sidecars.mjs --release`.
- `desktop/scripts/prepare-sidecars.mjs` (new) — resolves the host target
  triple (`rustc -vV`), locates or builds `valori-daemon`/`valori-node`
  (release binaries always rebuilt for `--release`; dev mode reuses whatever
  already exists, building debug only if nothing does), copies them into
  `src-tauri/binaries/<name>-<triple>[.exe]` — the naming convention Tauri's
  bundler requires.
- `desktop/scripts/dev.mjs` (new) — the new `beforeDevCommand`: runs
  `prepareSidecars({ release: false })` synchronously (required for **every**
  cargo build of the desktop crate, not just `tauri build` — Tauri's build
  script validates `externalBin` resource paths unconditionally, discovered
  when a stale `tauri dev` session hit `resource path
  binaries/valori-daemon-aarch64-apple-darwin doesn't exist` after the
  `externalBin` config was added), then starts `ui/`'s dev server as the
  long-running foreground process `tauri dev` expects.
- `desktop/src-tauri/src/daemon_manager.rs` — rewritten around a
  `RunningChild` enum (`Dev(tokio::process::Child)` /
  `Sidecar { CommandChild, exited: Arc<Notify> }`):
  - **Exactly two code paths**, switched on `cfg!(debug_assertions)`: dev
    searches `target/{release,debug}` under the repo root (unchanged from
    D3); release spawns via `tauri_plugin_shell::ShellExt::sidecar()`. The
    `VALORI_DAEMON_BIN` env-var override from D3 is removed, per spec.
  - **Version handshake**: after health passes, `GET /version` is checked
    against a compile-time-expected daemon API level (`"v1"`, the `api` field
    — not the raw crate semver, which isn't a compatibility contract). A
    mismatch kills the just-spawned child and returns an actionable
    `UnsupportedVersion: desktop expects daemon api "v1", daemon reports
    "..."` error instead of a later, mysterious failure.
  - **`valori-node` bundling actually wired through**: bundling the node
    binary alone does nothing — `valori-daemon`'s own `LocalRuntime::resolve_binary()`
    only knows how to find `valori-node` via `VALORI_NODE_BIN` or a
    `target/{release,debug}` search, neither of which exists on an end
    user's machine. `sidecar_sibling_path("valori-node")` (replicates
    `tauri-plugin-shell`'s private `relative_command_path` — same-directory-
    as-current-exe, with the "deps" test-binary adjustment — since that
    resolution isn't a public API) computes the bundled node sidecar's path
    and passes it as `VALORI_NODE_BIN` when spawning the daemon sidecar.
  - `start_daemon`/`stop_daemon` commands now take `app: tauri::AppHandle` in
    addition to `State<DaemonState>` (needed for `app.shell()`).
- `desktop/src-tauri/Cargo.toml` — added `tauri-plugin-shell = "2"`.
- `desktop/src-tauri/src/lib.rs` — registers `tauri_plugin_shell::init()`.
- `desktop/.gitignore` — `src-tauri/binaries/` (generated per-platform,
  never committed).

## Findings

- Tauri's `externalBin` resource-path check runs on **every** cargo build of
  the crate that declares it — dev included — not just `tauri build`. Adding
  the config without also prepping sidecars before `tauri dev` broke a live
  dev session mid-conversation (`resource path binaries/valori-daemon-...
  doesn't exist`). Fixed by moving sidecar prep into `beforeDevCommand`
  itself (`scripts/dev.mjs`), run synchronously before the long-running `ui/`
  dev server starts.
- `tauri-plugin-shell::process::Command::new_sidecar` resolves the binary
  path relative to the *running app's own executable*, not through Tauri's
  asset/resource API — confirmed by reading the crate source directly
  (`relative_command_path` in `tauri-plugin-shell-2.3.5/src/process/mod.rs`).
  This is why dev mode can't reuse the sidecar mechanism at all: the desktop
  crate is a standalone Cargo workspace with its own `target/`, separate from
  the root workspace's `target/` where `valori-daemon`/`valori-node` actually
  build — so `sidecar()` would never find them in dev, hence the two
  genuinely separate code paths the user asked for.
- Bundling `valori-node` without wiring `VALORI_NODE_BIN` would have been a
  no-op in production — the daemon has no built-in awareness that it's
  running inside a Tauri bundle. Found and fixed while implementing "bundle
  node too," not called out explicitly in the request.

## Validation

- `cargo build` / `cargo build --release` in `desktop/src-tauri` — both
  clean, both compile **both** `RunningChild` arms (the `cfg!(debug_assertions)`
  branch is a runtime check, not `#[cfg]`, so the sidecar-spawn code path is
  type-checked on every build even though it isn't exercised by the current
  tests).
- `cargo test` in `desktop/src-tauri` — 2 passed: `version_compat_check`
  (pure function, both match and mismatch cases) and
  `start_stop_real_daemon_binary_dev_path` (real dev-mode spawn: health,
  version check, graceful shutdown via `/v1/shutdown`, confirmed exit).
- `node scripts/prepare-sidecars.mjs` and `node scripts/prepare-sidecars.mjs
  --release` both run end-to-end: resolved `aarch64-apple-darwin`, built (or
  reused) both binaries, produced correctly-named files under
  `src-tauri/binaries/`.

## Follow-ups

- **The `RunningChild::Sidecar` spawn path itself is not exercised by an
  automated test.** `tauri-plugin-shell`'s sidecar resolution is relative to
  the *running app's own binary* — reproducing that in a `cargo test` run
  would require either a fully packaged `.app` or a `tauri::test::mock_app()`
  harness with the module made generic over `tauri::Runtime` (the commands
  are currently pinned to the concrete default `AppHandle`/`Wry` type). Given
  the API is read directly from the linked crate source and matches
  documented, widely-used Tauri usage, this was judged disproportionate
  effort for now; flagging as a real gap rather than silently accepting it.
- **A real `tauri build` was not run** — blocked by a pre-existing,
  out-of-scope issue: `ui/` doesn't statically export yet (`frontendDist:
  ../ui/out` requires `next build --output export`, but `ui/` still uses
  server-side API routes). This is explicitly Phase 3 territory per the
  user's own sequencing, not something this phase touches. Once static
  export lands, a real `tauri build` + install is the natural remaining
  verification step for the sidecar path end-to-end.
- Windows `.exe` handling in `sidecar_sibling_path` / `prepare-sidecars.mjs`
  is implemented but untested (no Windows machine in this environment).
- D1.3 (installers: `.dmg`/`.msi`/`.AppImage`) is next per the user's own
  stated sequencing — mostly packaging work now that sidecar bundling is in
  place, not architecture work.
