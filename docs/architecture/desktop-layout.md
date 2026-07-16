# Desktop app & workspace directory layout

Reference for contributors working on `desktop/` — what's actually inside a
built app bundle, and what's inside a user's chosen workspace (`VALORI_HOME`).
See [`control-plane.md`](control-plane.md) for who owns what at a higher
level, and [`../phases/phase-D3.1-sidecar-bundling.md`](../phases/phase-D3.1-sidecar-bundling.md) /
[`../phases/phase-D1.3-installers.md`](../phases/phase-D1.3-installers.md) for how this came to be.

## The app bundle (macOS `Valori.app` shown; Windows/Linux differ only in
## packaging format, not in what's bundled)

```text
Valori.app/
└── Contents/
    ├── MacOS/
    │   ├── valori-desktop      # main Tauri binary — the only thing double-clicked
    │   ├── valori-daemon       # sidecar: project/workspace lifecycle
    │   ├── valori-node         # sidecar: spawned BY valori-daemon per project
    │   └── node                # sidecar: runs the bundled ui/ standalone server
    ├── Resources/
    │   ├── icon.icns
    │   └── ui-server/          # ui/'s `next build` (output: "standalone") —
    │       ├── server.js       # entry point run as: node server.js
    │       ├── node_modules/   # traced, minimal — not the full ui/ node_modules
    │       ├── .next/static/   # copied in manually (Next excludes it from
    │       │                   # standalone output by design — see
    │       │                   # scripts/prepare-ui-server.mjs)
    │       └── public/
    └── Info.plist
```

**Nothing here needs Node.js, Rust, or Cargo installed on the end user's
machine.** All four `Contents/MacOS/` executables are self-contained
platform binaries — three are genuine Rust binaries; `node` is a copy of the
Node.js runtime made at packaging time (`prepare-sidecars.mjs`), not a
system dependency.

**Why `node` is bundled as a sidecar but `ui-server/` is a resource, not
another sidecar:** a Tauri sidecar must be one self-contained executable.
`node` alone qualifies; `ui-server/` is a JS application (`server.js` +
`node_modules`) that needs an interpreter to run — hence `node` (sidecar) +
`ui-server/server.js` (resource), spawned together (`ui_server_manager.rs`
runs `<node sidecar> <path-to-resource>/server.js`).

## Startup sequence (release build)

```text
Launch Valori.app
  │
  ├─ Rust setup(): spawn `node ui-server/server.js` (PORT=17862)
  │     └─ poll http://127.0.0.1:17862 until healthy
  │           └─ navigate main window there (was showing a
  │              "Starting Valori…" loading page — src-tauri/loading/)
  │
  └─ (JS side, once the real UI has loaded)
        Welcome flow (first run) or AppShellGate (returning user)
        calls startDaemon(workspaceDir)
              └─ spawns the `valori-daemon` sidecar with VALORI_HOME=<workspaceDir>
                    and VALORI_NODE_BIN=<path to the bundled `valori-node` sidecar>
                       (the daemon has no idea it's inside a Tauri bundle —
                        this is how it finds valori-node without a Cargo/
                        target-dir search, which wouldn't exist on a real install)
```

## Workspace layout (`VALORI_HOME` — what the user picks in Welcome/Settings)

```text
<workspace>/
├── ui-projects.json          # legacy single-node registry (pre-daemon; migrated
│                              # in place, never deleted — see phase-B.0.5 docs)
├── projects.json              # daemon's project registry (name -> ProjectManifest)
├── workspaces.json            # daemon's workspace registry ("default" always exists)
└── projects/
    └── <project-name>/
        ├── project.json       # this project's manifest (dim, index kind, restart_policy, …)
        ├── events.log          # BLAKE3-chained audit log — the durability source of truth
        ├── snapshot.val        # periodic/on-shutdown snapshot (V6 format)
        └── node.log            # captured stdout/stderr from this project's valori-node
```

Each project directory is fully self-contained and portable — copying one
`projects/<name>/` directory to another machine's workspace and re-registering
it is the entire "migrate a project" story (see `valori-daemon`'s migration
framework for how this happens automatically for pre-daemon single-node
projects).

## Fixed ports (loopback only, desktop-internal)

| What | Port | Why fixed (not dynamically allocated) |
|---|---|---|
| Bundled `ui-server` (release builds) | `17862` | Only the desktop app itself ever talks to it — no conflict risk worth dynamic allocation for. See `ui_server_manager::UI_SERVER_PORT`. |
| `valori-daemon` | `8080` | `VALORI_DAEMON_BIND` default, unchanged from pre-desktop usage. |
| Each project's `valori-node` | `8100`–`8999` (dynamic) | Allocated by `valori-daemon`'s own `PortAllocator` — internal detail, never exposed to the client. |
