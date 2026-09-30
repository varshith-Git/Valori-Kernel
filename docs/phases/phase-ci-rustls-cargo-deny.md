# Phase CI — rustls cargo-deny audit fix

## Goal

Restore the failing `License & Security Audit (cargo-deny)` main-branch check
by addressing the RustSec advisory reported against the locked `rustls`
version, without broad dependency churn or policy relaxation.

## Delivered

| File | What landed |
|---|---|
| `Cargo.lock` | Updated `rustls` from `0.23.43` to `0.23.45`, the first non-vulnerable patch release for `RUSTSEC-2026-0285`. Cargo also normalized `tempfile`'s `getrandom` edge from `0.4.3` to the already-present `0.3.4`. |
| `docs/phases/phase-ci-rustls-cargo-deny.md` | This report. |
| `docs/phases/README.md` | Status-table row for the cargo-deny repair. |
| `CHANGELOG.md` | `[Unreleased]` note for the audit fix. |

No crate README or root `README.md` update was required: no crate source,
public API, endpoint, CLI command, env var, or user-visible behavior changed.

## Findings

- `cargo deny check` failed on `RUSTSEC-2026-0285`: `rustls 0.23.43` accepted
  some TLS 1.3 handshake messages across encryption-level boundaries.
- The fix was available as a patch-level lockfile update to `rustls 0.23.45`;
  no `deny.toml` advisory ignore was added.
- The audit still reports a non-failing warning for yanked `chacha20 0.10.1`
  through `lopdf -> rand`, but the cargo-deny gate now passes under the
  repository's current policy.

## Validation

- `cargo deny check` — passed (`advisories ok, bans ok, licenses ok, sources ok`),
  with the existing non-fatal yanked `chacha20 0.10.1` warning.
- `cargo test -p valori-kernel -p valori-node` — passed: 645 passed, 0 failed,
  2 ignored. The first sandboxed attempt failed before app logic because
  `api_as_of` could not bind `127.0.0.1:0`; the rerun with loopback binding
  permitted passed.

## Follow-ups

- Track the yanked `chacha20 0.10.1` warning through the `lopdf`/`rand`
  dependency chain and update when upstream releases a compatible non-yanked
  path.
