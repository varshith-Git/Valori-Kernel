# Phase: Studio Installation Identity

## Goal

Fix the bug found by `docs/reviews/installation-id-audit.md`: `installation_id`
was only ever generated as a side effect of the telemetry send path, so any
user who never opted into telemetry (the default state after onboarding)
never received an installation identity at all — contradicting its own
documented contract ("returns the exact same value forever across
launches"). Decouple installation-identity generation from telemetry
consent entirely, and consolidate the three independent get-or-init
implementations the audit found into one canonical implementation.

## Delivered

- **[`desktop/src-tauri/src/preferences_service.rs`](../../desktop/src-tauri/src/preferences_service.rs)**
  — `StudioPreferencesService::get_or_init_installation_id` is now the
  **sole canonical implementation**, documented as such. Return type
  changed from `String` to typed `InstallationId` (callers that need a
  string call `.to_string()`); `get_installation_id_command` updated
  accordingly. Added 7 new tests covering the fresh-install,
  telemetry-off (the critical regression), telemetry-on,
  stable-across-reopen, existing-id-preservation, and session-linkage
  invariants.
- **[`desktop/src-tauri/src/lib.rs`](../../desktop/src-tauri/src/lib.rs)**
  — `setup()` now calls `get_or_init_installation_id()` **unconditionally**,
  immediately after `StudioPreferencesService` is registered as managed
  state and before session start — independent of telemetry consent,
  Cloud login, or project state. Replaces the previous plain,
  non-initializing read. Logs a warning (never panics) if it fails,
  matching the existing "studio.redb is non-fatal" pattern documented on
  `init_studio_storage`.
- **[`desktop/src-tauri/src/telemetry.rs`](../../desktop/src-tauri/src/telemetry.rs)**
  — the private `installation_id()` helper no longer generates or writes
  anything itself; it now reads through
  `StudioPreferencesService::get_or_init_installation_id` (the same
  managed-state instance `lib.rs` already registers). Removed ~10 lines of
  duplicated get-or-init logic.
- **[`ui/src/lib/native.ts`](../../ui/src/lib/native.ts)** — no logic
  change (the Tauri branch was already a pure read; the browser fallback
  was already a genuinely separate, non-desktop code path). Doc comment
  expanded to explicitly state the desktop/browser separation and that
  `studio.redb` is the sole desktop persistence location.
- **[`desktop/src-tauri/tests/installation_id_architecture.rs`](../../desktop/src-tauri/tests/installation_id_architecture.rs)**
  (new) — 4 architecture tests: exactly one Rust generation site exists;
  no second desktop persistence file/table is referenced; the Tauri branch
  of `getInstallationId()` never touches `localStorage`; `lib.rs` and
  `session_service.rs` never generate ids themselves.
- **[`docs/architecture/studio-storage.md`](../architecture/studio-storage.md)**
  — new §19 "Installation identity" documenting the invariant, canonical
  storage/generation, session linkage, complete-loss behavior, and the
  architecture enforcement test.
- **[`docs/phases/README.md`](README.md)** — status row added.
- **[`CHANGELOG.md`](../../CHANGELOG.md)** — entry added under `[Unreleased]`.

**Explicitly not touched** (per the task's scope): credential/keychain
migration, telemetry redesign, session pruning, sync, marketplace, Cloud
analytics, model hosting. `InstallationId` remains an opaque UUID — no
email/user_id/organization_id/account_id/IP/device-serial/username was
added to it.

## Findings

- The three implementations were already mutually idempotent (all shared
  the same "write only if `None`" `redb` transaction guard), so no data
  corruption risk existed — the bug was purely one of *timing*: nothing
  ever called any of them for a telemetry-off user.
- Existing sessions recorded before this fix (with `installation_id:
  None`) cannot and should not be retroactively backfilled — doing so
  would fabricate history. They remain accurate records of what the app
  actually knew when they were created; the fix only guarantees the
  invariant going forward.
- One pre-existing, unrelated `cargo fmt` violation was found in
  `crates/valori-studio-storage/examples/dump_studio_db.rs` during
  verification — not touched, out of this phase's scope (surgical-change
  rule).
- The desktop crate (`valori-desktop`) has 26 pre-existing
  `clippy::result_large_err` warnings and 1 pre-existing
  `clippy::assert_eq_literal_bool` warning, unrelated to this phase and
  present before it (confirmed via `git stash`). The task's required
  clippy scope was `valori-studio-storage` only, which is clean.

## Validation

```text
cargo build --lib                              (desktop crate)  clean
cargo test --lib                                (desktop crate)  41 passed, 0 failed
cargo test --test installation_id_architecture  (desktop crate)   4 passed, 0 failed
cargo fmt --check                               (workspace)      clean (except 1 pre-existing,
                                                                    unrelated file, not touched)
cargo check --workspace                                          clean
cargo test -p valori-studio-storage                             105 passed, 0 failed
cargo test --workspace                                          all green, 0 failures
cargo clippy -p valori-studio-storage --all-targets -- -D warnings  clean
cargo test -p valori-node --test dependency_direction --test architecture   7 passed, 0 failed
npx tsc --noEmit                                                 clean
npm run build                                                    succeeds
```

### Real desktop smoke test

Against a disposable `$VALORI_HOME=/tmp/valori-installation-id-test`
(deleted before and after), running the actual compiled
`desktop/src-tauri` binary:

| Scenario | Result |
|---|---|
| 1. Fresh install, telemetry never touched (defaults off) → launch | `installation_id` = `6b9d0a18-e59d-449c-9ad4-e4f90b2f4a2f` generated; `telemetry_consent` = `None` (fail-closed off) |
| 2. Restart | same id |
| 3. Enable analytics + restart | same id |
| 4. Disable analytics + restart | same id |
| 5. Session linkage (checked on every run above) | every session's `installation_id` equals `preferences.installation_id` |

All 5 scenarios passed. The disposable directory and scratch inspection
binaries used only for this smoke test were deleted afterward; nothing
from the smoke test is part of the shipped diff.

## Follow-ups

- None required by this phase's scope. The audit's other observations
  (item 8, backend telemetry ingestion at `api.valori.systems`) remain
  out of repo scope, unchanged.
- The pre-existing `cargo fmt` issue in `dump_studio_db.rs` and the
  pre-existing desktop-crate clippy warnings are flagged above but
  intentionally left for whichever phase next touches those files —
  fixing them here would be an unrelated source change.
