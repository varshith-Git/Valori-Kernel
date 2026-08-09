# Valori Studio — Installation ID Audit

**Status:** Read-only investigation. No source code was modified as part of this audit.
**Scope:** `crates/valori-domain/`, `crates/valori-studio-storage/`, `desktop/src-tauri/`, `ui/src/`.
**Trigger:** Live `~/.valori/studio.redb` inspection showed `preferences.installation_id = ""` with `telemetry.analytics = false`, `telemetry.crash = false`, and multiple valid sessions already recorded.

---

## 1. Summary verdict

> **After a user installs Valori Studio, is there exactly one stable anonymous installation identity that survives application restarts and upgrades?**

**No — not currently, for every user who leaves telemetry off.** The installation ID is only ever created as a *side effect* of the telemetry send path. If a user completes onboarding with the "Share anonymous usage telemetry" checkbox left unchecked (its default state), `installation_id` is never generated at all — not at first launch, not on any subsequent launch — until the moment telemetry is turned on and the next gated `send()` call actually fires. This directly contradicts the promise documented in `native.ts`:

```ts
/** A permanent id generated once per install, persisted in studio.redb alongside
 *  everything else — never tied to a Valori Cloud account or any other identity.
 *  Returns the exact same value forever across launches. */
export async function getInstallationId(): Promise<string> {
```

This is a **real design gap**, not merely a display artifact — see §5 for why.

---

## 2. Full flow as implemented today

```
                          ┌─────────────────────────────┐
                          │  App startup (lib.rs setup)  │
                          │  studio_db.preferences()     │
                          │    .get() -> installation_id │   <- PLAIN READ, never inits
                          └───────────────┬───────────────┘
                                          │ (Option<InstallationId>, possibly None)
                                          ▼
                          ┌─────────────────────────────┐
                          │ session_service.start_session│
                          │  stores installation_id      │   <- passthrough, no generation
                          │  (possibly None) on session   │
                          └───────────────────────────────┘

              (installation_id is otherwise NEVER touched by startup)

  ── Only reachable when telemetry consent is true ──────────────────────────
  ui: telemetry.ts::send()
     if (!nativeAvailable()) return;
     const installationId = await getInstallationId();   <- LAZY GET-OR-INIT #1 (JS wrapper)
        -> invoke("get_installation_id_command")
           -> preferences_service.rs::get_or_init_installation_id()   <- LAZY GET-OR-INIT #2 (Rust)
              if None: generates InstallationId::new(), stores if still None, returns
     await invoke("enqueue_telemetry_event", { installationId, category, ... })

  ── Separately, drain_queue (background sender) ────────────────────────────
  desktop/src-tauri/telemetry.rs::drain_queue
     (only runs past an early-return if the queue is non-empty)
     let install_id = installation_id(app).unwrap_or_default();  <- LAZY GET-OR-INIT #3 (private Rust fn)
        -> same get-or-init pattern, independent implementation
     -> installation_id: String is placed into the outgoing TelemetryEnvelope
     -> POST https://api.valori.systems/v1/telemetry/events (external Cloud endpoint,
        outside this repo's scope — no further verification possible from source alone)
```

Three independent call sites can generate the ID (all guarded by the same "only set if the redb field is `None`" pattern, so they're mutually idempotent, but they are duplicated logic — see §7, item 12). **None of them is reachable unless telemetry is enabled and a `send()`/`drain_queue` call actually executes.**

---

## 3. Answers to the 12 questions

**1. Where is the installation ID generated?**
`InstallationId::new()` (`crates/valori-domain/src/id.rs`, via the shared `uuid_id!` macro — generates a fresh UUID v4). It is invoked from three call sites, all under `desktop/src-tauri/`:
- `preferences_service.rs::get_or_init_installation_id` (public, backs the `get_installation_id_command` Tauri command)
- `telemetry.rs`'s private `installation_id(app)` helper (used only inside `drain_queue`)
- `ui/src/lib/native.ts::getInstallationId()`'s browser-only (non-Tauri) fallback branch, which generates client-side via `crypto.randomUUID()` — not relevant to the desktop app, only to the browser/Cloud build.

**2. Is it generated only once, or repeatedly?**
Logically once — every generation site checks `if p.installation_id.is_none()` inside a redb `update()` closure before writing, so a second caller racing in will not overwrite an already-set value. But it is generated *lazily and conditionally*, not unconditionally at startup (see Q5).

**3. Where is it persisted?**
`StudioPreferences.installation_id: Option<InstallationId>` (`crates/valori-studio-storage/src/preferences.rs`), stored in the `preferences` table of `studio.redb`.

**4. Is the persisted value supposed to live in `studio.redb`?**
Yes. `studio.redb` is Studio's single local metadata store (preferences, sessions, telemetry queue, etc. all live there — see `docs/architecture/studio-storage.md`), and `preferences.rs`'s own doc comment states this field "mirrors `ui/src/lib/native.ts`'s `getInstallationId()`" — i.e. it's the canonical, intended location.

**5. Does startup guarantee that an installation ID exists?**
**No.** `lib.rs`'s `setup()` (around line 477) does a plain, non-initializing read:
```rust
let installation_id = studio_db.preferences().get().ok().and_then(|p| p.installation_id);
```
There is no unconditional call to `get_or_init_installation_id()` anywhere in the startup path. The value is used as-is (possibly `None`) to start the session.

**6. Do sessions use the same installation ID?**
Sessions use *whatever installation_id existed in preferences at the moment the session started* — a passthrough, not a fresh lookup or generation (`session_service.rs::start_session` → `db.sessions().start(id, installation_id, ...)`). If preferences' installation_id was `None` at that moment (the common case for a telemetry-off user), the session record's `installation_id` is also `None`. Sessions created before the user ever opts into telemetry will permanently carry `None`, even after an installation_id is later generated — there is no retroactive backfill.

**7. Does telemetry use the same installation ID?**
Yes, once one exists — `telemetry.ts::send()` calls the same `getInstallationId()`/`get_or_init_installation_id()` path that preferences.rs owns, and `drain_queue`'s `installation_id(app)` helper reads/writes the identical `studio.redb` field. All three call sites converge on one shared value once any of them has run.

**8. Does the backend receive it?**
The outgoing `TelemetryEnvelope.installation_id: String` is POSTed to `TELEMETRY_ENDPOINT = "https://api.valori.systems/v1/telemetry/events"` (`desktop/src-tauri/src/telemetry.rs:95,384`). This is an external Valori Cloud endpoint outside this repository — its handling of the field cannot be verified from source in this repo and is out of scope for this audit.

**9. Does migration from older Valori versions handle it?**
Yes, for users who already had a legacy value. `crates/valori-studio-storage/src/migration.rs` parses a legacy `preferences.json`'s `installationId: Option<String>` field, validates it as an `InstallationId`, and writes it into `studio.redb`'s `preferences.installation_id` (lines ~111, 235-236, 269-270) during the one-time, idempotent, non-destructive legacy import. This only helps users who are upgrading from an old build that had already generated and persisted an installation id — it does not help a brand-new install.

**10. Why does the current live database contain an empty string?**
The observed live state (`analytics: false, crash: false`, multiple sessions, `installation_id: ""`) is consistent with: the user completed onboarding leaving the "Share anonymous usage telemetry" checkbox unchecked — `Welcome.tsx` defaults `const [telemetry, setTelemetry] = useState(false)` and, if left as-is, calls `await setTelemetryConsent({ analytics: telemetry, crash: telemetry })` with `telemetry = false` for both fields. Because `getInstallationId()` is called *only* from `telemetry.ts::send()`, and every caller of `send()` is gated by `if (!consent.analytics) return;` (or the equivalent crash-consent check), `send()` never executes, so `getInstallationId()` is never invoked, so `installation_id` is never written. Sessions are still created (session creation is unconditional and not gated by telemetry consent), which matches "multiple sessions exist" in the observed state. The literal `""` is most likely how the inspection tooling renders an absent/`None` `Option<InstallationId>` field, rather than a genuine empty-string value written by the app — `InstallationId` is `#[serde(transparent)]` over `Uuid`, and `Uuid` has no valid empty-string representation, so no code path in this repo can serialize a literal `""` into that field. This should be verified directly against the raw redb bytes if a fix is implemented, but no such literal-empty-string write site exists in the source.

**11. Can an empty/absent installation ID create duplicate or unstable telemetry identity?**
Yes, in two ways:
- **Instability across time within one install**: a user who starts with telemetry off (id absent) and later opts in gets an id generated *at that later point*. Sessions recorded before that point are permanently stamped with `None` and can never be attributed to the same identity as sessions/events recorded afterward — the "one stable identity" guarantee is broken for that install's own history.
- **No duplication risk today** because nothing currently sends telemetry with a missing id substituted by something non-unique — the `unwrap_or_default()` in `drain_queue` only converts `Option<String>` to `String` for the wire envelope, and by that point `installation_id(app)` has already lazily created a real UUID if one was missing (i.e., the `None` branch is only hit if the redb write itself somehow failed, an edge case, not the normal path). So there's no dupe/empty-string telemetry actually being sent today — but only because telemetry itself is what triggers the fix-up in the first place, which is circular: telemetry is off, so nothing ever triggers the id to exist, so there's nothing (yet) sent with a broken id. The risk materializes the moment any code (a future feature, a support/debug flow, a diagnostics export) reads `installation_id` expecting it to always be present and treats absence as "new install" — it would misclassify a long-lived, multi-session install as brand new.

**12. Are there multiple competing installation-ID implementations?**
Yes — three independent lazy get-or-init implementations exist (see §2 diagram): `preferences_service.rs::get_or_init_installation_id` (Rust, public/Tauri-command-backed), `telemetry.rs`'s private `installation_id()` helper (Rust, only reachable from `drain_queue`), and `native.ts::getInstallationId()`'s browser-only fallback (TypeScript, irrelevant to desktop but a third duplicate pattern for the Cloud/browser build). All three are mutually idempotent (same "write only if still `None`" redb-transaction guard) so they don't corrupt each other, but the duplication is unnecessary maintenance surface — see §7 recommended fix.

---

## 4. Actual source files involved

| File | Role |
|---|---|
| `crates/valori-domain/src/id.rs` | Defines `InstallationId` (UUID newtype via `uuid_id!` macro) |
| `crates/valori-studio-storage/src/preferences.rs` | `StudioPreferences.installation_id: Option<InstallationId>` — the persisted field |
| `crates/valori-studio-storage/src/session.rs` | `StudioSessionRecord.installation_id: Option<InstallationId>` — passthrough copy on each session |
| `crates/valori-studio-storage/src/migration.rs` | Legacy `preferences.json` → `studio.redb` migration, including `installationId` |
| `desktop/src-tauri/src/lib.rs` (~line 477) | Startup: plain read of installation_id, passed into session start — no init |
| `desktop/src-tauri/src/preferences_service.rs` | `get_or_init_installation_id()` + `get_installation_id_command` Tauri command |
| `desktop/src-tauri/src/telemetry.rs` | Second independent get-or-init (`installation_id()` fn); `TELEMETRY_ENDPOINT`; envelope construction in `drain_queue` |
| `desktop/src-tauri/src/session_service.rs` | `start_session()` passthrough of installation_id into `db.sessions().start(...)` |
| `ui/src/lib/native.ts` | `getInstallationId()` — Tauri command wrapper + browser-only fallback |
| `ui/src/lib/telemetry.ts` | The *only* caller of `getInstallationId()`; gated by consent inside `send()` |
| `ui/src/components/onboarding/Welcome.tsx` | Telemetry checkbox defaults to `false`; sets `{analytics, crash}` consent on completion; never calls `getInstallationId()` |
| `ui/src/components/settings/SettingsModal.tsx` | Post-onboarding consent toggle (`setTelemetryConsent`); also never calls `getInstallationId()` — toggling consent on does not itself trigger id generation, only the next gated `send()` does |

---

## 5. Bug or expected state?

**Bug (design gap).** The documented contract in `native.ts` ("Returns the exact same value forever across launches") and the field's evident purpose (a stable, anonymous, non-PII install identifier — explicitly "never tied to a Valori Cloud account or any other identity") both imply this identity should exist independent of telemetry opt-in. Coupling its generation to the telemetry send path is very likely unintentional: the id itself carries no telemetry data and isn't sensitive, so there's no privacy reason to gate its *creation* (as opposed to its *transmission*) behind consent. The current behavior means the observed live state (`""`/absent id, telemetry off, real sessions) is not a corrupted or unusual case — it is the guaranteed outcome for any user who doesn't opt into telemetry, which is likely the majority given the checkbox defaults to unchecked.

---

## 6. Backward-compatibility implications

- **Existing installs with telemetry already off**: have zero installation_id history. Once a fix generates the id unconditionally at startup, that id will be brand new — there is no way to reconstruct "what it would have been" for past sessions, because it was never computed.
- **Existing sessions already recorded with `installation_id: None`**: cannot be retroactively backfilled with a real id after the fact without either (a) leaving them `None` forever (historical gap, acceptable) or (b) a one-time migration that stamps old sessions with the newly-generated id, which would be **incorrect** — it would imply those old sessions always had that identity when they didn't. Recommend leaving historical `None` sessions as-is; only fix forward.
- **Legacy migration path (`migration.rs`)** already correctly forward-migrates any pre-existing `installationId` from `preferences.json`, so upgrading users who *did* have telemetry on under an old build are unaffected by this gap.
- **No wire-format change needed**: `Option<InstallationId>` already round-trips fine through serde whether `Some` or `None`; a fix does not require a schema/version bump.

---

## 7. Recommended fix (NOT implemented — audit only)

1. Call the existing `get_or_init_installation_id()` **unconditionally** during startup in `lib.rs`'s `setup()`, independent of telemetry consent — right where the current plain read happens (~line 477), replacing the read-only `.and_then(|p| p.installation_id)` with the get-or-init call. This is the minimal change: the function already exists and is idempotent.
2. Consolidate the three duplicate get-or-init implementations into one shared function (`preferences_service.rs`'s version is the natural canonical one, since it's already public and service-layer-owned) and have `telemetry.rs`'s `installation_id()` call it instead of re-implementing the same redb pattern.
3. Leave `native.ts`'s browser-only fallback branch as-is (it's a genuinely separate code path for the non-Tauri/Cloud build, not a bug).
4. Do not attempt to backfill historical `None` sessions (see §6).

## 8. Tests that should be added

- **Startup guarantee test**: fresh `studio.redb` (no prior preferences), telemetry consent left at default `{false, false}`, start the app/session flow → assert `preferences.installation_id` is `Some(_)` after startup completes (currently it would be `None` — this test would fail today, demonstrating the gap).
- **Session/telemetry identity consistency test**: after startup, assert every new session's stored `installation_id` equals `preferences.installation_id`, and that the id used in a subsequently-enqueued telemetry envelope also matches — proving one shared identity across all three surfaces once the fix lands.
- **Idempotency test**: call the get-or-init path twice in a row (simulating the three independent call sites) → assert the second call returns the exact same UUID as the first, never regenerates.
- **Legacy migration regression test** (should already exist, verify coverage): a `preferences.json` with a pre-existing `installationId` migrates that exact value into `studio.redb`, and startup does *not* generate a new one on top of it.
- **Historical-session non-backfill test**: a session created before the fix (with `installation_id: None`) is not mutated by a later fix/migration — it stays `None` permanently.

---

*End of audit. No source files were modified. Awaiting approval before any implementation work begins.*
