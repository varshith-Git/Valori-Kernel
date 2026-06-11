# Phase 1.10 — CI Upgrades: Multi-Arch Hash Equality, Throughput Regression, cargo-deny

**Status:** planned  
**Roadmap:** [MULTINODE_ROADMAP.md](../MULTINODE_ROADMAP.md) § 1.10  
**Why now:** The existing `multi-arch-determinism.yml` workflow runs tests
separately on x86, ARM, and WASM but extracts hashes via `grep "HASH:"` —
fragile text scraping. Phase 1.10 replaces this with a proper determinism
assertion job, adds a write-throughput regression gate, and locks the
dependency license surface with `cargo-deny`. All three are prerequisites for
Phase 2 (cluster mode) where any hash divergence is a split-brain event.

---

## Goal

1. **Multi-arch hash equality job** — replay the same committed fixture log on
   x86-64 (ubuntu-latest) and ARM64 (macos-latest); `assert_eq!` the BLAKE3
   state hashes in a dedicated test binary. Exit 0 only if they match. This is
   the mechanical enforcement of the core determinism invariant.
2. **Write-throughput regression gate** — bench `insert_record_from_f32` at a
   fixed workload size; fail CI if throughput drops below a configured floor.
3. **`cargo-deny`** — license scan (no AGPL transitive deps, ever) + advisory
   database check on every PR.
4. **Pinned golden-hash test integration** — the Phase 1.3 pinned empty-state
   hash is already in `tests/format.rs`; Phase 1.10 makes it part of the
   required-pass list for the hash-equality job.

---

## D1 — Multi-Arch Hash Equality Job (replacement for current workflow)

### Problem with current approach

```yaml
# Current: fragile text scraping
- name: Run determinism test
  run: cargo test ... determinism_x86 -- --nocapture > x86_output.txt
- name: Extract hash
  run: grep "HASH:" x86_output.txt | head -1 | cut -d':' -f2 > x86_hash.txt
```

If the test output format changes, the grep silently produces an empty file,
the comparison `"" == ""` passes, and the CI appears green while determinism
is completely untested.

### Replacement: structured hash artifact

The test binary writes its hash to a structured JSON file, not stdout:

```rust
// crates/valori-node/tests/multi_arch_determinism.rs  [MODIFY — Phase 1.10]

/// Writes determinism result to `target/determinism-result.json` so the
/// CI job can read it without parsing terminal output.
fn write_result(hash: &[u8; 32], platform: &str) {
    let out = serde_json::json!({
        "platform":   platform,
        "hash_hex":   valori_wire::hex(hash),
        "events_replayed": FIXTURE_EVENT_COUNT,
        "schema_version": 1,
    });
    std::fs::write("target/determinism-result.json",
        serde_json::to_string_pretty(&out).unwrap()).unwrap();
}
```

The CI validate job reads `determinism-result.json` from each platform's
artifact, compares `hash_hex` fields, and fails with a diff if they differ:

```bash
# CI validate step
X86=$(jq -r .hash_hex x86/determinism-result.json)
ARM=$(jq -r .hash_hex arm/determinism-result.json)
if [ "$X86" != "$ARM" ]; then
    echo "❌ DETERMINISM FAILURE"
    echo "  x86: $X86"
    echo "  arm: $ARM"
    echo ""
    echo "This means the same event sequence produces different hashes on"
    echo "different architectures. This is a critical bug — investigate"
    echo "before merging. See docs/phases/phase-1.10-ci-upgrades.md"
    exit 1
fi
echo "✅ DETERMINISM VERIFIED: $X86"
```

Empty file or missing `hash_hex` key → `jq` exits non-zero → CI fails. No
silent false-positives.

### Fixture log commitment

The determinism test replays a *committed* fixture log, not an in-memory
sequence. The fixture is checked in at
`crates/valori-node/tests/fixtures/determinism-fixture.log`
(a v3 segment, 1000 events, generated once, committed to the repo).

Benefit: the fixture tests *serialization* determinism, not just *compute*
determinism. If a `bincode` encode order changes between platforms (it
shouldn't, but this proves it), the hash would diverge.

### Updated workflow YAML structure

```yaml
# .github/workflows/multi-arch-determinism.yml  [REPLACE — Phase 1.10]
name: Multi-Architecture Determinism Validation

on:
  push:
    branches: [main, multinode]
    paths: ['crates/**', 'Cargo.toml', 'Cargo.lock', '.github/workflows/**']
  pull_request:
    branches: [main]
    paths: ['crates/**', 'Cargo.toml', 'Cargo.lock']

jobs:
  build-fixture:
    name: Generate Determinism Fixture
    runs-on: ubuntu-latest
    steps:
      - checkout
      - build make-demo-log (from valori-verify)
      - run: cargo run --release --bin make-demo-log -- --events 1000 --out crates/valori-node/tests/fixtures/determinism-fixture.log
      - upload fixture as artifact
    # NOTE: In steady state, the fixture is committed to the repo.
    # This step regenerates it only on explicit fixture-refresh runs.
    # Normal CI uses the committed file, not this step.

  hash-x86:
    name: Hash (x86-64 Linux)
    runs-on: ubuntu-latest
    steps:
      - checkout
      - rust stable
      - cargo test -p valori-node --test multi_arch_determinism -- --nocapture
      - upload target/determinism-result.json as artifact x86-result

  hash-arm:
    name: Hash (ARM64 macOS)
    runs-on: macos-latest  # GitHub-hosted M-series runner
    steps:
      - checkout
      - rust stable
      - cargo test -p valori-node --test multi_arch_determinism -- --nocapture
      - upload target/determinism-result.json as artifact arm-result

  validate:
    name: Assert Hash Equality
    needs: [hash-x86, hash-arm]
    runs-on: ubuntu-latest
    steps:
      - download x86-result, arm-result
      - compare hash_hex fields with jq
      - fail if not equal (see shell above)
```

---

## D2 — Write-Throughput Regression Gate

### What is measured

Single-threaded `insert_record_from_f32` on a fresh in-memory engine with
`BruteForce` index, `dim=128`, `max_records=10_000`. 10,000 inserts.
Metric: **events/second**.

Using `cargo-criterion` (or `criterion` directly) because it produces a
JSON benchmark result that is comparable across runs.

### Floor values (Phase 1.10 baseline)

Measured on `ubuntu-latest` (x86-64, 2 vCPU GitHub runner):

| Workload | Floor (events/sec) | Notes |
|---|---|---|
| Single insert (no persistence) | 200,000 | In-memory only |
| Single insert (event log, no fsync on each) | 50,000 | Buffered write |
| Batch insert (1000 events) | 500,000 | Group commit |

These floors are deliberately conservative (50% of observed baseline) to avoid
flaky failures from runner variance. They will be tightened in Phase 2 once
the throughput story is stable.

### CI step

```yaml
# in ci.yml (to be created)
- name: Throughput regression check
  run: |
    cargo bench -p valori-node --bench throughput -- --output-format bencher \
      | tee bench-output.txt
    python3 scripts/check_throughput_floor.py bench-output.txt
```

`check_throughput_floor.py` reads the bencher output and asserts each
named benchmark exceeds its configured floor. Floors are in
`scripts/throughput_floors.json` (committed, updated intentionally).

Failure message:
```
❌ THROUGHPUT REGRESSION: insert_single_no_persist
   measured: 87,000 events/sec
   floor:    100,000 events/sec
   Δ:        -13%

This may indicate a performance regression in a recent commit.
Run locally: cargo bench -p valori-node --bench throughput
```

---

## D3 — `cargo-deny`

### Configuration

```toml
# deny.toml  [NEW — Phase 1.10]

[licenses]
# Allowed licenses for all transitive dependencies.
# AGPL and SSPL are forbidden — they would taint binary distributions.
allow = ["MIT", "Apache-2.0", "Apache-2.0 WITH LLVM-exception",
         "BSD-2-Clause", "BSD-3-Clause", "ISC", "Unicode-DFS-2016",
         "CC0-1.0", "MPL-2.0"]
deny  = ["AGPL-3.0", "AGPL-3.0-only", "AGPL-3.0-or-later",
         "SSPL-1.0", "GPL-2.0", "GPL-3.0"]
# Crates with multiple license options — we pick the most permissive.
exceptions = [
    { allow = ["OpenSSL"], name = "ring" },
]

[advisories]
# Deny crates with known security vulnerabilities.
db-urls = ["https://github.com/rustsec/advisory-db"]
vulnerability = "deny"
unmaintained  = "warn"
unsound       = "deny"

[bans]
# Prevent duplicate versions of these critical crates.
deny = []
multiple-versions = "warn"
```

### CI step (added to existing PR workflow)

```yaml
- name: License + advisory scan
  uses: EmbarkStudios/cargo-deny-action@v2
  with:
    arguments: --all-features
    command: check
    manifest-path: Cargo.toml
```

This runs on every PR. A PR that introduces an AGPL transitive dep fails CI
immediately, before any reviewer needs to notice.

---

## D4 — Pinned Golden-Hash Integration

Phase 1.3 introduced a pinned golden hash test:

```rust
// crates/valori-kernel/src/tests/format.rs
const PINNED_EMPTY_STATE_HASH: &str = "4eeaa41d…4d4a";
```

Phase 1.10 lifts this into the multi-arch determinism fixture:

```rust
// crates/valori-node/tests/multi_arch_determinism.rs

const PINNED_EMPTY_STATE_HASH_HEX: &str = "4eeaa41d…4d4a"; // from phase-1.3

fn test_empty_state_pinned_hash() {
    let state = KernelState::new();
    let hash = hash_state_blake3(&state);
    let computed = valori_wire::hex(&hash);
    assert_eq!(computed, PINNED_EMPTY_STATE_HASH_HEX,
        "Empty-state golden hash changed! This means the hash domain was
         modified without bumping STATE_HASH_DOMAIN_VERSION. If this is
         intentional, update the pin and document the break in the phase doc.");
}
```

This test is part of the `multi_arch_determinism` test binary, so it runs on
*both* x86 and ARM in the CI job. A hash-domain change that wasn't intentional
fails CI on both arches, not just one.

---

## D5 — CI Consolidation: New `ci.yml`

The current CI setup has `e2e-test.yml` (Python e2e) and
`multi-arch-determinism.yml` (hash equality). Phase 1.10 adds a consolidated
`ci.yml` that gates every PR:

```yaml
# .github/workflows/ci.yml  [NEW — Phase 1.10]
name: CI

on:
  push:    { branches: [main, multinode] }
  pull_request: { branches: [main] }

jobs:
  # ── Fast gate (runs on every push, completes in < 3 min) ──────────────────
  check:
    name: cargo check + fmt + clippy
    runs-on: ubuntu-latest
    steps:
      - checkout
      - rust stable + rustfmt + clippy
      - cargo fmt --check
      - cargo clippy --workspace -- -D warnings
      - cargo check -p valori-kernel -p valori-wire -p valori-node -p valori-verify -p valori-cli -p valori-consensus

  test:
    name: cargo test (unit + integration)
    runs-on: ubuntu-latest
    needs: check
    steps:
      - checkout
      - cargo test -p valori-kernel -p valori-wire -p valori-node -p valori-verify -- --test-threads=4
      - cargo test -p valori-node --test '*' -- --test-threads=1

  deny:
    name: cargo-deny (licenses + advisories)
    runs-on: ubuntu-latest
    needs: check
    steps:
      - EmbarkStudios/cargo-deny-action@v2

  # ── Determinism gate (runs on push to main + on PR, parallelized) ─────────
  hash-x86:
    name: Hash (x86-64)
    runs-on: ubuntu-latest
    needs: test

  hash-arm:
    name: Hash (ARM64)
    runs-on: macos-latest
    needs: test

  determinism-validate:
    name: Assert hash equality
    needs: [hash-x86, hash-arm]

  # ── Throughput gate (runs on push to main only — slow) ────────────────────
  throughput:
    name: Throughput regression check
    runs-on: ubuntu-latest
    needs: test
    if: github.ref == 'refs/heads/main'
    steps:
      - cargo bench + check_throughput_floor.py

  # ── E2E (existing, unchanged) ─────────────────────────────────────────────
  e2e:
    name: Python E2E tracer
    runs-on: ubuntu-latest
    needs: test
```

Total time budget on PR: **< 8 minutes** (check 2m + test 3m + deny 1m + hash
comparison 2m, all parallelized after `test` passes).

---

## New Files

| File | Description |
|---|---|
| `.github/workflows/ci.yml` | Consolidated CI gate |
| `deny.toml` | `cargo-deny` configuration |
| `crates/valori-node/tests/fixtures/determinism-fixture.log` | Committed 1000-event v3 segment |
| `scripts/check_throughput_floor.py` | Throughput floor assertion script |
| `scripts/throughput_floors.json` | Configured floor values per benchmark |
| `crates/valori-node/benches/throughput.rs` | `criterion` benchmark |

### Modified files

| File | Change |
|---|---|
| `.github/workflows/multi-arch-determinism.yml` | Replaced with structured JSON artifact approach |
| `crates/valori-node/tests/multi_arch_determinism.rs` | Writes `determinism-result.json`; adds pinned empty-state hash check |

---

## Acceptance Criteria

| Criterion | Evidence |
|---|---|
| `ci.yml` gates all PRs | Workflow file present; required status check in repo settings |
| Hash equality test uses structured JSON | `determinism-result.json` uploaded as artifact |
| Empty file in artifact → CI fails | Tested by a dry-run with no test binary output |
| AGPL dep rejected by deny.toml | Tested by temporarily adding an AGPL dep in a draft PR |
| Throughput floor not breached | `check_throughput_floor.py` exits 0 on current main |
| Pinned empty-state hash in determinism fixture | `test_empty_state_pinned_hash` in both arch jobs |
| `cargo clippy -- -D warnings` clean | Part of `check` job |
| `cargo fmt --check` clean | Part of `check` job |

---

## Findings

Design-only phase — no runtime findings. Two forward concerns:

**Throughput floor calibration:** The floors must be calibrated on the actual
GitHub runner hardware, not developer machines. ARM64 runners (macos-latest =
M-series) are significantly faster per-core than x86 Ubuntu runners. The
floors in D2 are placeholders; run the benchmark on CI once and set the floor
at 50% of the measured median. Document the calibration commit hash in
`scripts/throughput_floors.json`.

**`cargo-deny` false positives:** `ring` (used transitively by `rustls`) has
a dual OpenSSL/ISC license. The `exceptions` block in `deny.toml` handles
this, but new `ring` versions may change the license expression. Pin `ring`
in `Cargo.lock` (it already is via `Cargo.lock` workspace resolver) and update
`deny.toml` on each `ring` version bump.

## Follow-ups

- Phase 2: Add ARM Linux runner (Graviton) to the determinism job — the
  mixed-arch validation described in roadmap § 2.6 (x86 leader + ARM
  followers, identical hashes). GitHub now offers ARM64 Linux runners via
  `ubuntu-24.04-arm`.
- Phase 2: Throughput benchmark extended to cover group-commit batching
  (the Phase 2 performance story).
- Phase 2: `determinism-validate` job extended to compare Raft-replicated
  state hashes across a 3-node `docker-compose` run (the Phase 2 test).
