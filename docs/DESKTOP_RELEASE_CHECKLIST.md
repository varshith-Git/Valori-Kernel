# Desktop release checklist

Manual verification steps for a Valori Desktop build — not automated yet
(Phase D1.3). Run this before calling a build "shippable," on each target
platform if possible; at minimum on the platform you changed.

## Build

- [ ] `cd desktop && npm run build` completes with no errors (bundles
      `valori-daemon` + `valori-node` + `node` as sidecars, `ui/` as a
      resource — see [`docs/architecture/desktop-layout.md`](architecture/desktop-layout.md)).
- [ ] The installer for your platform exists under
      `desktop/src-tauri/target/release/bundle/{dmg,msi,appimage}/`.

## Clean-machine install (the test that actually matters)

Ideally a clean VM/user account with no Rust, no Node.js, no repo checkout —
proves the "zero toolchain, zero terminal" claim, not just "it works on the
machine that built it."

- [ ] Install the built package (mount+drag the `.dmg`, run the `.msi`, mark
      the `.AppImage` executable and run it).
- [ ] Launch — the window should show a brief "Starting Valori…" loading
      page, then switch to the real dashboard within a few seconds.
- [ ] Welcome flow appears (first run). Choose a workspace folder.
- [ ] Create a project.
- [ ] Create a collection inside it.
- [ ] Insert or upload a document/record.
- [ ] Search / confirm the record is retrievable.
- [ ] Quit the app (window close button, not force-quit).
- [ ] Relaunch. Confirm: no Welcome flow this time (onboarding persisted),
      the same project/collection/document are still there (state survived
      a real process restart — proves snapshot-on-shutdown + daemon
      auto-relaunch with the persisted workspace).

## What this proves, end to end

| Step above | What it actually exercises |
|---|---|
| Loading page → dashboard | Bundled `node` sidecar + `ui-server` resource + window navigation |
| Welcome → workspace chosen | `VALORI_HOME` wiring (folder picker has a real effect, not cosmetic) |
| Create project | `valori-daemon` sidecar spawning `valori-node` via `VALORI_NODE_BIN` |
| Insert / search | The full data path through the spawned node |
| Quit → relaunch, data intact | Graceful shutdown (snapshot-then-terminate) + recovery on the daemon's next start |

If any step fails, it's very likely one of: a missing sidecar (build didn't
bundle correctly), a stale `VALORI_NODE_BIN` path (bundling changed but the
wiring didn't), or a snapshot/shutdown regression — check
`~/.valori/projects/<name>/node.log` and the daemon's own logs first.

## Known gaps (Phase D1.3)

- No automated version of the above yet — this is a documented manual
  checklist, deliberately not scripted (per phase scope).
- Windows `.msi` / Linux `.AppImage` are built by CI
  (`.github/workflows/desktop-build.yml`) but have not been run through this
  checklist on real Windows/Linux machines — only macOS has been manually
  verified end-to-end as of this writing.
- Builds are unsigned and unnotarized — expect Gatekeeper/SmartScreen
  warnings on install. Signing + notarization is Phase D1.4.
