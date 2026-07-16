## Summary

What does this change do, and why?

## Determinism Impact

- [ ] No determinism impact
- [ ] Improves determinism
- [ ] Changes arithmetic behavior (Q16.16 / `FxpScalar`)

If arithmetic behavior changes, explain why and what compatibility fixtures were updated.

## `no_std` / Kernel ABI

Only applies if `crates/valori-kernel` or `crates/valori-core` were touched.

- [ ] N/A — kernel/core not touched
- [ ] No `use std::` added inside `valori-kernel/src/`; std-only code is gated behind `#[cfg(feature = "std")]`
- [ ] `cargo build -p valori-kernel --target wasm32-unknown-unknown` passes locally

## Standalone + Cluster Parity

Only applies if this adds or changes an HTTP endpoint or a `KernelEvent`.

- [ ] N/A — no endpoint/event change
- [ ] Handler added to **both** `server.rs` and `cluster_server.rs`
- [ ] Write path in cluster mode goes through `raft.client_write()`, not a direct engine lock
- [ ] `cargo test -p valori-node --test route_parity` passes (or the allowlist was updated with a reason)

## Cross-Architecture Behavior

Tested on:
- [ ] x86
- [ ] ARM / Apple Silicon
- [ ] Embedded / Jetson

## Tests

- [ ] `cargo test -p valori-kernel -p valori-node` passes
- [ ] `cargo fmt --all -- --check` and `cargo clippy --workspace --all-targets -- -D warnings` pass

Include reproduction / validation cases below.

## UI (if applicable)

- [ ] N/A — no UI change
- [ ] Verified in both dark and light mode (semantic tokens only, no hardcoded colors)

## Notes

If this relates to forensic or evaluator tracks, confirm that logic is kernel-appropriate.

Anything deferred to a follow-up, and which phase/issue owns it.
