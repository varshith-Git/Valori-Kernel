# Phase SH-H1 — Shared Free-tier hosting: live end-to-end verification

## Goal

Verify, with real evidence (not code inspection alone), whether Valori's
existing shared Free-tier hosting (`crates/valori-node/src/shared.rs`)
actually delivers the intended model — one shared worker process hosting
multiple Free projects, each with an independent `Engine`, WAL, and state
root, with no per-project container — end to end from a fresh Docker build
through registration, isolation, restart, and lifecycle operations.

## Delivered

- Full read-only architecture trace of `crates/valori-node/src/shared.rs`,
  `main.rs`, and `docs/shared-hosting.md` against the actual checked-out
  code, with file/line evidence for every claim.
- 4 new regression tests added to `crates/valori-node/tests/shared_hosting.rs`,
  closing isolation-coverage gaps the existing 5 tests left open:
  `shared_reactivation_restores_serving_and_preserves_state`,
  `shared_live_writes_do_not_cross_project_state_roots`,
  `shared_projects_isolate_second_collection_with_different_config`,
  `shared_event_log_proof_is_reachable_and_project_scoped`.
- A real production Docker image built from the repository's own
  `Dockerfile` and run as a disposable shared-hosting worker, with two
  disposable projects registered, populated, restarted, suspended, and
  reactivated.
- `docs/reviews/shared-free-hosting-live-verification.md` — the full
  verification report (architecture trace, test tables, isolation matrix,
  container evidence, failure paths, confirmed limitations, launch
  recommendation).

## Findings

- **Blocker (new, not previously tracked):** the Cloud control plane
  (`backend/apps/api` and `valori-ui`'s Next.js app) has **zero**
  integration with shared hosting. No `SHARED_WORKER_*` env var is read
  anywhere, no `0021_shared_projects.sql` migration exists, no
  `/v1/admin/orgs/{id}/dedicate` endpoint exists, and every project —
  Free or paid — is provisioned through the same dedicated-container
  `Provisioner::deploy()` path. `docs/shared-hosting.md`'s claim that "the
  Cloud control plane chooses shared hosting for new free projects" does
  not match the checked-out code.
- **Deployment gap:** running the production Dockerfile's distroless
  nonroot image against a fresh Docker named volume for
  `VALORI_SHARED_ROOT` fails every project registration with `Permission
  denied (os error 13)` — the volume defaults to root ownership, which the
  nonroot image can't write to. Undocumented in `docs/shared-hosting.md`.
  Worked around for this verification with a `chmod 777` bind mount; not
  fixed in code (out of scope for a verification task per the assignment's
  own rules).
- Everything else tested (credential isolation, collection isolation,
  record/graph/metadata isolation, live and restart-boundary state-root
  isolation, suspend/reactivate/delete lifecycle, container-count
  invariant) passed with no code changes required — the node-level
  implementation is correct.
- All limitations already tracked in
  `docs/superpowers/specs/2026-09-12-shared-hosting-hardening-design.md`
  (eager loading, no eviction, count-based admission, missing per-project
  limits, corrupt-log blast radius) were independently reconfirmed against
  live behavior, not just static code reading.

## Validation

- `cargo test -p valori-node --test shared_hosting -- --nocapture`:
  **9/9 passed** (5 pre-existing + 4 new), 0 failed.
- `cargo test -p valori-kernel -p valori-node`: **641 passed, 0 failed, 2
  ignored**.
- `cargo fmt --check`: pass.
- `cargo clippy -p valori-kernel -p valori-node --all-targets -- -D
  warnings`: pass, 0 warnings.
- Real Docker: built `valori-node:shvfy-test` from the repo `Dockerfile`;
  ran as a disposable `VALORI_SHARED_ROOT` worker; registered two
  disposable projects; verified full isolation matrix; restarted the
  container and confirmed byte-identical `final_state_hash` for both
  projects; suspended and reactivated one project without affecting the
  other. Full detail and exact commands/evidence in
  `docs/reviews/shared-free-hosting-live-verification.md`.
- Control-plane (Stage 5) and Azure (Stage 6) verification: **not
  performed** — no code path exists to exercise (control plane) and no
  credentials were available (Azure). Not a test failure; a missing
  integration and an environment constraint, respectively.

## Follow-ups

- **P0:** build the Cloud→shared-worker control-plane integration
  (`backend/apps/api/src/provision/`) — this phase found the gap but did
  not implement it, per this task's explicit instruction to verify first
  and not implement fixes without separate authorization.
- **P0:** document/fix the non-root shared-root volume permission issue
  (`docs/shared-hosting.md`, `Dockerfile`).
- **P1:** Phase 4A (failure quarantine) from the hardening roadmap, before
  any production rollout — a single corrupt project log still blocks the
  entire worker's startup.
- **P1:** Phase 1 (benchmark/audit harness) of the same roadmap, now
  informed by this verification's confirmed gaps.
- **P2:** repeat Stage 5 (control-plane E2E) and Stage 6 (Azure) once the
  P0 integration exists / credentials are available.
