# Phase P8 — CI Hardening

## Goal

Complete the CI quality gate with two missing jobs — line/branch coverage reporting and Miri
undefined-behaviour detection — and make the shared `rust-setup` composite action fully
configurable so it can serve all job types without repetition.

## Delivered

| File | What landed |
|---|---|
| `.github/workflows/ci.yml` | +`coverage` job (llvm-cov, lcov artifact, job-page summary) + `miri` job (nightly, fxp + proof suites) |
| `.github/actions/rust-setup/action.yml` | +`toolchain` input (default `stable`), +`components` input — composite action now covers stable, nightly, and component-specific jobs |

## CI job map (complete after P8)

| Job | Workflow | Blocks merge | What it checks |
|---|---|---|---|
| `fmt` | `ci.yml` | yes | `cargo fmt --all -- --check` |
| `clippy` | `ci.yml` | yes | `cargo clippy --workspace --all-targets -D warnings` |
| `test` | `ci.yml` | yes | `cargo test -p valori-kernel -p valori-node` |
| `route-parity` | `ci.yml` | yes | standalone ↔ cluster route parity test |
| `ui-typecheck` | `ci.yml` | yes | `tsc --noEmit` on `ui/` |
| `coverage` | `ci.yml` | no (informational) | llvm-cov line/branch % on valori-kernel; lcov artifact + job summary |
| `miri` | `ci.yml` | yes | UB detection on `tests/fxp.rs` + `tests/proof.rs` under Miri |
| `wasm-build` | `kernel-abi.yml` | yes (path-filtered) | `valori-kernel` + `valori-core` compile for `wasm32-unknown-unknown` |
| `regression` | `write-regression.yml` | soft-fail | p99 insert latency + batch throughput vs baseline |
| `cargo-deny` | `cargo-deny.yml` | yes | dependency audit (advisories, licenses, bans) |
| `count-tests` | `test-count.yml` | no (main only) | badge update |

## Coverage job design

- Uses `taiki-e/install-action@cargo-llvm-cov` to install a prebuilt `cargo-llvm-cov` binary
  (avoids the ~3-minute compile of the tool from source).
- Runs `cargo llvm-cov --package valori-kernel --lcov` — node crate excluded; its tests are
  mostly integration tests requiring a live server, which inflate noise and mask kernel signal.
- Uploads `lcov.info` as a 14-day artifact — importable into VS Code Coverage Gutters,
  `genhtml`, or any LCOV-compatible viewer.
- Writes a `--summary-only` table to `$GITHUB_STEP_SUMMARY` — visible on the Actions job page
  without needing to download the artifact.
- Does **not** gate on a minimum threshold; the baseline (36.24% post-K3) is tracked in
  `docs/phases/phase-K3-coverage-audit.md` and raised manually as tests are added.

## Miri job design

- Uses nightly toolchain + `miri` component via the updated `rust-setup` action.
- `MIRIFLAGS=-Zmiri-disable-isolation` lets Miri handle any incidental syscalls from
  the Rust standard library without aborting (blake3 uses `core::`, not `std::`, so this
  flag is a no-op for the proof tests, but keeps the job robust).
- **Scope: `tests/fxp.rs` + `tests/proof.rs`** — chosen because:
  - Both are pure computation (Q16.16 arithmetic / Merkle BLAKE3) with no I/O or threads.
  - `blake3` is compiled with `default-features = false` (no SIMD), so it runs as pure Rust
    under Miri without hitting uninterpreted intrinsics.
  - These are exactly the modules where integer UB (overflow, wrong shift, bad cast) would be
    hardest to catch with regular tests but easy to catch with Miri.
- **Not scoped**: `snapshot_version_migration.rs` (42-second test under normal runner; Miri
  would be 10×+), `search.rs` / `bq_eval.rs` (SIMD-adjacent code paths).

## Findings

- The existing `rust-setup` composite action used `dtolnay/rust-toolchain@stable` with a
  hardcoded channel. Switching to `@master` with `toolchain: ${{ inputs.toolchain }}` is the
  documented pattern for a configurable channel; all existing callers default to `stable` and
  are unaffected.
- `taiki-e/install-action` is the standard way to install cargo-llvm-cov in CI — it downloads
  a prebuilt binary instead of compiling, saving ~3 minutes per run.
- Coverage is not run on `valori-node` because the node tests spin up HTTP servers and hit
  actual kernel state — they provide real regression value in the `test` job but generate
  misleading coverage numbers (handler boilerplate covers easily, kernel internals not at all).

## Validation

No local execution possible (CI-only jobs). Validated by:
- `python3 -c "import yaml; yaml.safe_load(open('ci.yml'))"` — both files parse cleanly.
- Manual review: job names, step order, action inputs, env var names all consistent with
  existing jobs in the same file.

## Follow-ups

- **Codecov integration** — upload lcov to `codecov/codecov-action` once `CODECOV_TOKEN` is
  configured; add `fail_ci_if_error: true` once a stable threshold is established.
- **Coverage threshold gate** — add `--fail-under 40` (or similar) once the K3→K4→P6 coverage
  gains are measured and a new baseline is committed.
- **Expand Miri scope** — add `tests/crypto.rs` (crypto-shredding state machine) once it is
  confirmed to be Miri-clean; add `tests/snapshot_roundtrip.rs` behind a test-name filter to
  avoid the slow V1–V7 chain test.
- **P7** — WAL validation tests (owns the `read_entry()` EOF bug found in P5).
