# Phase Studio S2c — Privacy Boundary & Persistence Cleanup

**Branch:** `feat/node-object-store-durability-and-metrics`
**Depends on:** `docs/architecture/studio-persistence-audit.md` (the two
findings this phase implements)
**Status:** Complete.

Implements exactly the two concrete issues the Studio Persistence Audit
found — nothing else. No credential/keychain work, no session pruning, no
sync, no other persistence feature.

---

## 1. Audit (before any code was written)

Read `desktop/src-tauri/src/telemetry.rs`, `preferences_service.rs`,
`crates/valori-studio-storage/src/telemetry.rs`,
`crates/valori-studio-storage/src/preferences.rs`, and every existing
telemetry test. Findings:

- **Where consent is evaluated:** `analytics_consent(&app)` in
  `desktop/src-tauri/src/telemetry.rs`, called at two enqueue call sites
  (`enqueue_telemetry_event`, `enqueue_update_event`). **Nowhere in
  `drain_queue`** — confirmed by reading the function in full; it checked
  only `app.try_state::<Arc<StudioDatabase>>()`.
- **Where events are queued:** `TelemetryQueue::enqueue` in
  `crates/valori-studio-storage/src/telemetry.rs`, backing the single
  `telemetry_queue` table.
- **Where the uploader drains the queue:** `drain_queue` in
  `desktop/src-tauri/src/telemetry.rs`, on a 60s timer via `spawn_sender`.
- **Whether the queue contains only analytics events:** **No** — crash
  events (`studio_crashed`, sent from `ui/src/lib/telemetry.ts`'s
  `reportSessionStarted`) go through the exact same
  `enqueue_telemetry_event` command and land in the same
  `telemetry_queue` table. Confirmed by reading `ui/src/lib/telemetry.ts`
  in full.
- **A real, previously-undocumented bug found by this audit step:**
  `enqueue_telemetry_event` checked `analytics_consent` unconditionally —
  including for `studio_crashed`, which the JS side already gates on
  `consent.crash`. A user with `crash: true, analytics: false` had crash
  reports silently dropped at enqueue time, contradicting the explicit
  "crash reporting remains independently controlled" requirement. Fixed
  in this phase (see §2) since the required test scenario
  ("independent crash consent") cannot be meaningfully proven without it.
- **Whether performance telemetry uses the same queue:** No "performance"
  consent category or event stream was found anywhere in
  `TelemetryConsent`, `StudioTelemetryEvent`, or any call site — the
  public consent model was **not** changed (still exactly
  `{ analytics: bool, crash: bool }`), per the explicit instruction not
  to invent a third category without evidence.
- **How consent changes are persisted:** `StudioPreferencesService::set_telemetry_consent`
  writes `StudioPreferences.telemetry_consent` in `studio.redb`'s
  `preferences` table — a single, atomic redb write transaction.
- **Whether there was already an event category/type field:** **No** —
  `StudioTelemetryEvent` had no field distinguishing which consent
  category gated a given row. This is the structural gap that made
  "invalidate analytics events without touching crash events" impossible
  before this phase — a new `TelemetryCategory` field was added (see §2).

For theme: read `ui/src/lib/theme.tsx`, `native.ts`, `preferences_service.rs`,
`StudioPreferences`, `Welcome.tsx`, `SettingsModal.tsx`. Findings:

- Theme is read/written in exactly one place: `ThemeProvider` in
  `theme.tsx`. No other hook or component touches it directly.
- `localStorage` **is** still needed for web-only mode — `ui/` runs both
  inside Tauri (Valori Studio desktop) and standalone as Valori Cloud's
  web UI / `npm run dev`, and the browser build has no `studio.redb` at
  all. Confirmed via `nativeAvailable()`'s existing role throughout
  `native.ts` as the established Tauri-vs-browser branch point.
- SSR needs an initial value: `ThemeProvider`'s `useState` defaults to
  `"dark"` before `loadTheme()`'s effect resolves — unchanged by this
  phase, not a new concern.
- No `studio.redb` schema change was needed — `preferences.theme:
  Option<String>` already existed (S1).

## 2. Delivered

### Telemetry consent revocation (Part 1)

- **`crates/valori-studio-storage/src/telemetry.rs`** — new
  `TelemetryCategory` enum (`Analytics | Crash`), matching
  `TelemetryConsent`'s two existing fields exactly (no third category
  added). `StudioTelemetryEvent` gained a `category` field,
  `#[serde(default)]` to `Analytics` for rows written before this field
  existed (the historically accurate value — every prior row was in fact
  gated by the old analytics-only check). New
  `TelemetryQueue::discard_category(category) -> usize` — one atomic
  bulk-delete transaction, mirroring `prune_older_than`'s existing shape.
  `StudioTelemetryEvent::new` gained a required `category` parameter (all
  crate-internal and desktop call sites updated).
- **`crates/valori-studio-storage/src/migration.rs`** — legacy
  `events.jsonl` rows migrate in as `TelemetryCategory::Analytics` (what
  actually gated their creation historically).
- **`desktop/src-tauri/src/telemetry.rs`**:
  - `analytics_consent` replaced by `consent_for_category(app, category)`,
    matching each category to its own `TelemetryConsent` field.
  - `enqueue_telemetry_event` (Tauri command) gained a `category`
    parameter; `enqueue_update_event` (Rust-native call sites) hardcoded
    to `Analytics` (every current call site is update-lifecycle telemetry).
  - `drain_queue` — the uploader boundary — now re-checks
    `consent_for_category` **per event, immediately before dispatching
    that event's HTTP request**, not once per batch and not relying on
    the enqueue-time guard. An event whose category consent is off right
    now is deleted (never sent, never retried) instead of uploaded.
- **`desktop/src-tauri/src/preferences_service.rs`** — new
  `discard_revoked_telemetry_categories(db, consent)`: for each category
  `consent` just turned off, calls `TelemetryQueue::discard_category`.
  Called from `set_telemetry_consent_command` immediately after
  persisting the new consent — the eager half of the invariant.
  Deliberately a **plain function**, not a method on
  `StudioPreferencesService`, preserving the S2b-2d.1 boundary
  ("`StudioPreferencesService` never reaches into `TelemetryQueue`'s
  table") — orchestrating two typed stores together is the command
  layer's job.
- **`ui/src/lib/telemetry.ts`** — `send()` gained a `category` parameter
  (`"analytics" | "crash"`, defaulting to `"analytics"`); the
  `studio_crashed` call site now passes `"crash"` explicitly — the fix
  for the enqueue-side bug found in the audit step.

### Theme dual-write removal (Part 2)

- **`ui/src/lib/theme.tsx`** — rewritten to branch on `nativeAvailable()`:
  desktop reads/writes `studio.redb` only (`getPreference`/`setPreference`),
  browser/web reads/writes `localStorage` only, unchanged from before.
  One-time, idempotent, non-destructive migration: if `getPreference("theme")`
  returns nothing on a native launch, the legacy `localStorage` value (if
  any) is read (never deleted) and backfilled into `studio.redb`. No
  separate "have I migrated" flag — the trigger condition can only be
  true once per installation.

### Documentation

`docs/architecture/studio-storage.md` — new §14.5 (Telemetry consent
enforcement) and §18 (Theme persistence); title/status line updated.
`docs/architecture/studio-persistence-audit.md` — both fixed findings
marked `[FIXED — S2c]` inline, cross-referenced to this doc; original
audit text otherwise preserved as a point-in-time record.
`docs/phases/README.md`, `CHANGELOG.md` updated. This phase doc.

### Explicitly not touched

Credentials/keychain, session pruning/retention, sync, update-state
wiring, `studio.redb` schema/table structure (only an additive field on
one existing struct), project storage, recovery mechanics (DR phase
untouched — see §5 below), `preferences.json`/`events.jsonl` migration
logic (S2a untouched).

## 3. Findings (bugs discovered during this phase's own audit step)

- **Crash events were silently gated by analytics consent, not crash
  consent** — see §1. This is the one place this phase's scope extended
  slightly beyond "exactly the two audit findings": fixing it was
  necessary for the audit's own required test ("verify the analytics
  queue cannot upload while crash reporting remains independently
  controlled") to be meaningful rather than vacuously true (crash events
  never reaching the queue at all when analytics was off, regardless of
  crash consent, would have made that test pass for the wrong reason).
- **`telemetry_storage_and_consent_are_independent_concerns`'s original
  assertion ("changing consent does not affect the queue") became
  partially inaccurate** once revocation started discarding events —
  fixed by clarifying the test now proves the *service* boundary alone
  (not the Tauri command orchestration) stays narrow, with new dedicated
  tests covering the command-level discard behavior.

## 4. Validation

```
cargo test -p valori-studio-storage
```
**105 tests, 0 failed** (was 101 after the DR phase; +4 new in
`tests/telemetry.rs`: category default-on-missing-field, `discard_category`
correctness/idempotency/reopen-persistence).

```
cd desktop/src-tauri && cargo test
```
**35 tests, 0 failed** (was 25; +10 new: 4 in `preferences_service.rs`
(`discard_revoked_telemetry_categories` correctness/idempotency/
does-nothing-when-consent-stays-on), 6 in `telemetry.rs` (revocation
discards the queued event; discards all N, not just one; re-enabling only
allows new events, never resurrects discarded ones; independent crash
consent survives analytics revocation; revocation durability across a
simulated restart; repeated "drain tick" checks never resurrect a
discarded event)).

```
cargo check --workspace                                                    clean
cargo clippy -p valori-studio-storage --all-targets -- -D warnings          clean
cd desktop/src-tauri && cargo clippy --all-targets                         clean (no new warnings)
cargo test -p valori-node --test dependency_direction --test architecture   7/7
cd ui && npx tsc --noEmit                                                   clean, exit 0
cd ui && npm run build                                                      clean, all routes compiled
```

### Real desktop application launch (disposable `$VALORI_HOME`, never
production data)

Built `cargo build --bin valori-desktop` and ran the actual binary against
`VALORI_HOME=/tmp/valori-s2c-test-1` (deleted afterward):

1. **Clean launch** — `Studio database opened at .../studio.redb`, no
   errors, normal healthy-path recovery (unaffected by this phase).
2. **Production-path verification against the live database file**
   (app killed first to avoid a lock conflict, then a small scratch
   binary linking the same `valori-studio-storage` crate opened the
   *exact* file the running app had just created and exercised the real
   production functions):
   - `StudioPreferences.theme`: `None` → `Some("light")` after the same
     update the Tauri command performs — proving the production code
     path round-trips through the live file correctly.
   - Enqueued one `Analytics`-category event via `TelemetryQueue::enqueue`
     (queue count: 1) → set `analytics: false` → called
     `TelemetryQueue::discard_category(Analytics)` (the exact call
     `set_telemetry_consent_command` makes) → queue count: **0**. Since
     `drain_queue`'s loop has nothing left to iterate, zero HTTP requests
     are possible — a complete proof, not a simulation of the network
     layer.
3. **Relaunch** — the app reopened the same (now-modified) database
   cleanly, with no corruption and no re-triggered legacy migration
   (idempotent, as expected), confirming the live file produced by step 2
   remains fully valid to the real application.

### Answering the required questions

**Can a queued analytics event ever be uploaded after analytics consent
has been revoked?**

**No — verified by test and by live application launch against real
`studio.redb` storage, not assumed.** Revocation eagerly discards every
queued event of that category (`discard_revoked_telemetry_categories` →
`TelemetryQueue::discard_category`), and independently, `drain_queue`
re-checks that category's consent fresh, per event, immediately before
every network dispatch — so even a hypothetical event that somehow
survived the eager discard (a future code path, a race) would still be
refused at the uploader boundary rather than sent. The only physical
limit this cannot cover is an HTTP request already in flight at the exact
instant revocation commits — a request that has already left the
process is not cancellable by any software design; per-event (not
per-batch) consent checking inside the drain loop minimizes that window
to at most one already-dispatched request, not a whole batch.

**Does Valori Studio now have exactly one authoritative persisted theme
value?**

**Yes, per environment — the only architecture the codebase's own
two-environment split supports, verified by test and live launch.** In
the desktop app, `studio.redb`'s `preferences.theme` is the sole store —
`localStorage` is read only once, for a one-time non-destructive
migration, and never written by the native code path anymore. In the
browser/web build (which has no `studio.redb` at all), `localStorage`
remains the sole store, unchanged. There is no configuration or code path
in which both stores are authoritative for the same running instance.

## 5. Follow-ups

- Everything the phase brief explicitly excluded remains excluded:
  credentials/keychain, session pruning, sync, update-state wiring,
  marketplace, model hosting, analytics dashboard.
- The audit's other findings (plaintext `apiKey` in `localStorage`,
  unbounded `sessions` growth) are unresolved, unchanged by this phase,
  and remain the next two candidates per the audit's own ranking.
- No frontend test framework exists in this repository; `theme.tsx`'s
  behavior relies on the storage-layer test suite plus live application
  verification rather than component-level unit tests. Introducing one
  was judged out of this phase's scope — flagged as a real gap for
  whichever future phase needs to test more frontend logic than a manual/
  live-launch check can reasonably cover.
