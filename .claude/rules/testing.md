# Testing rules

Verified against `Makefile`, `.github/workflows/ci.yml`, and `.github/workflows/*` for the rest. Every
command below is real — copy it, don't approximate it.

## Commands (verified)

```bash
make check                                          # cargo check --workspace --exclude valori-embedded --exclude valori-ffi
make test                                            # cargo test  --workspace --exclude valori-embedded --exclude valori-ffi
cargo test -p valori-kernel -p valori-node           # the two crates CI's `test` job actually runs
cargo test -p valori-node --test route_parity        # standalone/cluster route-set equality (mechanical)
cargo test -p valori-node --test api_contract        # ErrorCode enum vs. committed OpenAPI YAML
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo miri test -p valori-kernel --test fxp          # UB check — fixed-point arithmetic
cargo miri test -p valori-kernel --test proof        # UB check — Merkle root / receipt path
cargo build -p valori-kernel --target wasm32-unknown-unknown   # no_std/wasm boundary didn't break
cd ui && npx tsc --noEmit -p .
cd ui && npm run build
./scripts/api-contract-gate.sh                       # full API contract verification, one entry point
```

## Change → required verification matrix

| Change touches | Minimum required |
|---|---|
| `valori-kernel` algorithm (index, math, graph) | `cargo test -p valori-kernel`; if arithmetic — `cargo miri test -p valori-kernel --test fxp` |
| WAL / snapshot / hashing / receipts | `cargo test -p valori-kernel -p valori-node`; `cargo miri test -p valori-kernel --test proof`; see `.claude/rules/determinism.md`'s table for which invariant to re-check |
| Any HTTP endpoint | `cargo test -p valori-node --test route_parity` **and** `--test api_contract`; both `server.rs` and `cluster_server.rs` touched (`.claude/rules/architecture.md`) |
| `valori-kernel` public API, dependency, or `#[cfg]` gating | `cargo build -p valori-kernel --target wasm32-unknown-unknown` |
| Anything in `ui/**` | `cd ui && npx tsc --noEmit -p .` at minimum; `npm run build` for anything touching layout/routing/build config; visual check in both themes for anything touching color/layout (`.claude/rules/frontend.md`) |
| New/changed dependency anywhere in the Cargo workspace | `cargo deny check` |
| Cluster behavior (Raft membership, replication) | the relevant `docs/CLUSTER.md` manual scenario, plus `cargo test -p valori-node` — cluster integration tests aren't a single fast command; see that doc |
| Cross-machine determinism (arithmetic, hashing changed) | `.github/workflows/multi-arch-determinism.yml` covers this on CI — don't skip re-running it if you touched anything in the determinism table above |

## What "done" means

State exactly which subset you ran and why — "tests pass" without naming the crate/test filter is not
verification, it's a claim. If you ran `cargo test -p valori-kernel` but the change also touched
`valori-node`, say so and either run the second suite or explain why it's out of scope for this change.

## Test tiers that exist in this repo (don't invent others)

- **Unit / integration** — standard `cargo test`, scoped per-crate.
- **Mechanical contract tests** — `route_parity`, `api_contract`: these are equality checks against a
  generated artifact, not judgment calls. A failure means a real drift, not a flaky test to retry.
- **Miri (UB)** — `fxp` and `proof` only, by design (miri is slow; these two are where UB would silently
  corrupt deterministic state). Don't run all of `valori-kernel` under miri as a substitute for real tests.
- **Multi-arch determinism** — CI-only (`multi-arch-determinism.yml`), cross-compiles and diffs hashes
  across architectures. Can't meaningfully run locally; don't claim you verified this without CI.
- **E2E** — `.github/workflows/e2e-test.yml`, `e2e/` dir, `scripts/e2e_cluster_test.sh`.
- **Benchmarks** — see `.claude/skills/benchmark/SKILL.md`. Not a substitute for correctness tests; a
  benchmark proving a change is faster says nothing about whether it's still correct or deterministic.
- **SDK coverage/repro** — `scripts/sdk-coverage-check.py`, `scripts/sdk-repro-check.py` — verify the
  Python SDK actually reaches every endpoint it claims to, and that examples in its docs still run.
