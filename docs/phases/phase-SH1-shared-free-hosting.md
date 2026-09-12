# SH1 — Shared free-project hosting

## Goal

Host free Cloud projects in one worker process without provisioning a container
per project. Preserve dedicated standalone and cluster deployment paths for paid
projects and isolate every free project's collections and durable state.

## Delivered

- `crates/valori-node/src/shared.rs`: authenticated project registration,
  isolated engine/router/log state, persistent pause/resume, scoped deletion,
  corruption checks, admission bound, and log-based export/import for promotion.
- `crates/valori-node/src/main.rs`, `lib.rs`, `Cargo.toml`: opt-in shared boot
  mode, module export, and router dispatch dependency. No kernel changes.
- `crates/valori-node/src/server.rs`, `api_keys.rs`: private standalone transfer
  route with admin scope and a separate bounded request budget. The Raft path
  deliberately cannot import state outside consensus; public `/v1` routes and
  SDK contracts are unchanged.
- `crates/valori-node/tests/shared_hosting.rs`: cross-project isolation,
  authentication, lifecycle, restart, corruption, transfer, and tampering tests.
- `docs/shared-hosting.md`, root/node READMEs, `AGENTS.md`, `CHANGELOG.md`:
  operating instructions, new configuration, limitations, and rollout steps.
- Coordinated Cloud changes live in `D:/saas/valori-ui`; its SH1 phase report
  records plan routing, lifecycle, billing callback, binding migration, and UI.

## Findings

- Collection-name prefixes alone would not isolate graph IDs, proof surfaces,
  metadata, caches, and whole-node operations. Separate project engines share
  the process while preserving those boundaries.
- Forwarding an Axum request without clearing outer route captures caused ID
  handlers to return 500. The isolation regression caught this; dispatch now
  removes outer captures before invoking the project router.
- Whole-process snapshots have a documented mixed-dimension limitation; paid
  transfers therefore carry the event log plus sidecars and verify replay.
- Existing staged kernel/desktop/UI changes and the unresolved desktop merge
  conflict were left untouched. No `.env` file was read.

## Validation

- `cargo test -p valori-kernel -p valori-node`: **635 passed, 0 failed,
  2 ignored**, across 86 test/doc-test binaries. Includes the five new shared
  hosting tests and existing standalone/cluster route parity coverage.
- Cloud `cargo test --manifest-path backend/Cargo.toml`: **129 passed**.
- Cloud `tsc --noEmit`: passed. Targeted ESLint on all three changed TypeScript/
  TSX files: passed. `git diff --check`: passed in both repositories.
- Automated transfer test verifies the state proof before import, after import,
  on retry, and after reopening the dedicated engine from disk; tampered proofs
  and wrong credentials are rejected.

Manual deployment smoke test (not executed against Azure): create two free
projects with `documents`, insert distinct vectors, verify cross-token denial,
restart the shared worker, stop/delete one project, and confirm the other's
proof is unchanged. Upgrade the surviving project using Stripe test mode,
confirm dedicated routing only after import verification, and restart its
dedicated container to confirm persistence.

## Follow-ups

- SH2 rollout: apply the Cloud migration, deploy/configure the shared worker
  and updated paid-node image, and execute the two-account Azure/Stripe smoke
  test. No deployment was authorized or performed in this coding phase.
- SH2 capacity: benchmark the actual worker VM and choose admission limits;
  the default project count is not a performance guarantee.
- SH3 operations: streaming transfers beyond 256 MiB, shared-root backups/log
  retention, optional isolated embedding configuration/background work, and
  operator-approved migration of existing free dedicated projects.
- Shared mode intentionally blocks process administration and unsupported
  storage/crypto/background operations; see `docs/shared-hosting.md` for the
  exact scope. Existing dedicated behavior is preserved.
