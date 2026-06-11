# Phase 1.1 — Workspace restructure

**Status:** done · commit `2bd793d` on `multinode`
**Roadmap:** [MULTINODE_ROADMAP.md](../MULTINODE_ROADMAP.md) § 1.1

## Goal

Move every crate into a uniform `crates/` workspace so the seams Raft
needs (valori-wire, valori-consensus) have a home, with zero behavior
change. All moves via `git mv` — file history preserved.

## Delivered

```
crates/
  valori-kernel/      ← root src/ (logic untouched)
  valori-node/        ← node/
  valori-verify/      ← verify/
  valori-ffi/         ← ffi/
  valori-cli/         ← crates/cli
  valori-consensus/   ← NEW empty placeholder for Phase 2
embedded/             ← stays (Cortex-M firmware)
```

- Root `Cargo.toml` became a virtual workspace manifest.
- `default-members` excludes two crates that can never build on a plain
  host, with the reasons documented in the manifest:
  - `embedded` — `no_std` + its own panic handler (build with
    `--target thumbv7em-none-eabihf`)
  - `valori-ffi` — PyO3 extension module; links only when maturin builds
    it (`pip install ./python`)
- Updated: maturin `manifest-path`, CI workflow path filters,
  `tamper_demo.sh` repo-root resolution.
- Deleted: `src/tests/graph_tests.rs` (dead code — old const-generic
  API, never wired into the build), stray `my_report*.json`.

## Findings

1. **The CLI read the wrong wire format in production** — `replay-query`
   and `diff` decoded the pre-chain `LogEntry` shape and failed on event
   #1; `timeline` silently parsed garbage. Five integration tests were
   failing **on main before the restructure** (verified against a clean
   baseline worktree). This was wire-format drift #2 — the motivating
   evidence for Phase 1.2. Fixed by decoding `ChainedEntry`.
2. `cargo build --workspace` had never actually worked (embedded's panic
   handler collides with std on host targets) — hidden because nobody
   ran that exact command. Now explicit via `default-members`.

## Validation

- Full suite after restructure: **120 tests passing, 0 failures** across
  ~40 binaries (including all 9 CLI integration tests, 5 of which were
  failing before).

## Follow-ups

- The kernel's own unit tests (`src/tests/`) turned out to be entirely
  disconnected from the build → handled in Phase 1.1b.
- Three definitions of the wire format still existed (node, verify, cli)
  → collapsed in Phase 1.2.
