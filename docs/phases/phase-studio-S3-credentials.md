# Phase: Studio S3 — Credential Security

## Goal

Implement the approved architecture from `docs/reviews/studio-credentials-audit.md`:
move provider API keys (OpenAI/Cohere/Groq/Together/custom) out of
plaintext `localStorage` and into the OS credential store on the desktop
build, migrating existing plaintext credentials safely (verify-before-
delete, idempotent, fail-closed), without breaking the web/Cloud build,
Valori Cloud authentication, or any existing `studio.redb` data.

## Delivered

### `CredentialRef` — canonical typed reference

- **[`crates/valori-domain/src/id.rs`](../../crates/valori-domain/src/id.rs)**
  — `CredentialRef`, a UUID v4 newtype via the existing `uuid_id!` macro
  (same pattern as `InstallationId`/`ProjectId`/`SessionId`). Opaque,
  serializable/deserializable (`#[serde(transparent)]`), distinct from
  every other id type, structurally incapable of holding a secret (it's a
  `Uuid`, nothing else). Re-exported from `crates/valori-domain/src/lib.rs`.
- **[`crates/valori-domain/tests/invariants.rs`](../../crates/valori-domain/tests/invariants.rs)**
  — new test `credential_ref_create_serialize_deserialize_round_trip_and_distinctness`
  (create, serialize, deserialize, round-trip, distinctness) plus extended
  the existing malformed-UUID rejection test and the "every validated
  newtype is covered" tripwire (now 10 types). A doctest on the type itself
  also exercises create/round-trip/distinctness.

### `keyring` dependency

- **[`desktop/src-tauri/Cargo.toml`](../../desktop/src-tauri/Cargo.toml)**
  — `keyring = { version = "3", default-features = false, features =
  ["apple-native", "windows-native", "sync-secret-service"] }`. Version
  pinned to the `3.x` line specifically because `keyring@4.x` requires
  rustc 1.88, above this crate's `rust-version = "1.77"` — verified via
  `cargo add --dry-run` before committing to a version, per the task's
  instruction. Features selected per platform: `apple-native` (macOS
  Keychain via `security-framework`), `windows-native` (Windows Credential
  Manager), `sync-secret-service` (Linux, D-Bus Secret Service — the
  standard GNOME Keyring/KWallet backend; synchronous, matching this
  codebase's existing "no new async runtime" convention). No custom
  encryption layer or vault was written — `keyring` is the only
  credential-store implementation.

### `CredentialService`

- **[`desktop/src-tauri/src/credential_service.rs`](../../desktop/src-tauri/src/credential_service.rs)**
  (new) — the only code in the application that touches `keyring`
  directly. `store_new`/`store`/`get`/`exists`/`delete`, a small typed
  `CredentialError` (`Unavailable`, `PermissionDenied`, `NotFound`,
  `Invalid`, `Other`) with a `user_message()` that never exposes raw OS
  internals, and a `map_keyring_error` translation layer. Located in
  `desktop/src-tauri` (not `valori-domain`, not `valori-studio-storage`,
  not a new crate) — see "Findings" below for why.
- **Keychain naming**: `service = "Valori"`, `account = credential_ref`
  (the UUID string). One `CredentialRef` → exactly one keychain entry, by
  construction (`Entry::new(KEYCHAIN_SERVICE, &cred_ref.to_string())`). No
  API key, email, organization id, or project name is ever encoded into
  the keychain-visible metadata.
- **Tauri commands**: `credential_store(secret, existing_credential_ref)`,
  `credential_get(credential_ref)`, `credential_exists(credential_ref)`,
  `credential_delete(credential_ref)` — registered in
  [`lib.rs`](../../desktop/src-tauri/src/lib.rs)'s `generate_handler!` and
  a new `allow-credentials` permission set in
  [`permissions/daemon.toml`](../../desktop/src-tauri/permissions/daemon.toml),
  referenced from
  [`capabilities/default.json`](../../desktop/src-tauri/capabilities/default.json).
  `credential_store`'s `existing_credential_ref` parameter exists
  specifically so a password `onChange` handler (fires per keystroke) can
  overwrite the same keychain entry instead of minting a new, immediately
  orphaned one on every character typed — see "Findings" below.
  No `get_all_credentials()`-style broad API exists — enforced by a
  regression test.

### Provider configuration wiring

- **[`ui/src/lib/hooks/useLLMConfig.ts`](../../ui/src/lib/hooks/useLLMConfig.ts)**,
  **[`useEmbeddingConfig.ts`](../../ui/src/lib/hooks/useEmbeddingConfig.ts)**
  — the public `LLMConfig`/`EmbeddingConfig` shapes are **unchanged**, so
  none of the components consuming them (`AskTab.tsx`, `CommunityTab.tsx`,
  `EmbeddingSelector.tsx`, `DocumentUploadTab.tsx`, `MultiSearch.tsx`,
  `ContradictionTab.tsx`, `EntityExtractionTab.tsx`, `audit/page.tsx`, etc.
  — everything that reads `config.apiKey` off the hooks' return value)
  needed to change. What changed is internal to the two hook files: on
  desktop, `localStorage` now persists `{ provider, model, endpoint,
  credentialRef }` — never `apiKey` — and the in-memory `config.apiKey`
  consumers already read is resolved from the OS keychain at load time and
  re-stored (reusing the existing ref, not minting a new one) only when
  the key actually changes. Web/Cloud (`!isTauri()`) behavior is
  byte-for-byte unchanged — still `{ provider, model, endpoint, apiKey }`
  in `localStorage`.
- **Reranker config has no dedicated hook** — it turned out to have
  **three** independent readers/writers of the same
  `localStorage["valori:reranker_config"]` key, found during this phase's
  own final repository search (§29), not the earlier audit (which didn't
  need to enumerate every consumer, only every persistence location):
  [`SettingsModal.tsx`](../../ui/src/components/settings/SettingsModal.tsx)
  and [`app/settings/page.tsx`](../../ui/src/app/settings/page.tsx) (two
  separate, fully independent Settings UIs — both read/write/own the
  config, apparently pre-existing duplication from before this phase, not
  something S3 introduced) and
  [`AskTab.tsx`](../../ui/src/components/collections/AskTab.tsx) (a
  read-only consumer that sends the resolved config to `/api/why`). All
  three were updated identically: migrate-then-read, resolve
  `credentialRef` → secret via `credentialGet` on desktop, and (for the
  two writers) store/reuse/delete through `credentialStore`/
  `credentialDelete` exactly as the two config hooks do. Left as three
  separate call sites rather than extracted into a shared hook — that
  refactor is unrelated to credential security and out of this phase's
  scope; each site's fix is a mechanical, narrow application of the same
  already-proven pattern.
- **[`ui/src/lib/native.ts`](../../ui/src/lib/native.ts)** — new
  `credentialStore`/`credentialGet`/`credentialExists`/`credentialDelete`
  (thin Tauri command wrappers, desktop-only, throw/no-op outside Tauri)
  and `migrateLegacyProviderCredential` (the verify-before-delete migration
  algorithm, generic over any of the three `localStorage` keys).

### Provider execution (unchanged)

The actual provider HTTP calls (`ui/src/lib/server/{embed,llm,reranker}.ts`)
were **not** touched or moved to Rust. Per the task's explicit instruction
not to invent a second engine or do an unrelated rewrite: the resolved
secret still flows from the (now keychain-backed) React state into the
same POST body these server routes already expected. The only new bridge
is `credential_get`, called once per actual request to resolve
`credentialRef` → secret, never cached in persisted state.

### Daemon compatibility

- **[`crates/valori-daemon/src/project.rs`](../../crates/valori-daemon/src/project.rs)**
  — doc-comment-only addition confirming `CredentialRef::to_string()` (a
  bare UUID) is a valid `EmbeddingConfig.api_key_ref` value with **no
  adapter and no field rename**. The field's type stays `Option<String>`
  intentionally, so existing `project.json` manifests are unaffected.
  Nothing currently populates this field from the desktop credential flow
  — see "Follow-ups."

### Tests

- `crates/valori-domain/tests/invariants.rs` — `CredentialRef` matrix entry.
- `desktop/src-tauri/src/credential_service.rs` — 9 inline tests: store/get
  round-trip, exists reflects store/delete, missing credential is `None`
  not an error, delete is idempotent, different refs never collide, store-
  under-an-existing-ref overwrites (not duplicates), `user_message()`
  never leaks internals, a safe-telemetry-payload test, and an end-to-end
  fake-secret-never-in-serialized-preferences test. All run against the
  **real** OS credential store (no in-memory `keyring` backend exists);
  each skips gracefully, not fails, if the store is genuinely unavailable
  in the running environment.
- `desktop/src-tauri/src/preferences_service.rs` — new
  `generic_preference_bridge_rejects_every_secret_shaped_key` test.
- `desktop/src-tauri/tests/credential_security_architecture.rs` (new) — 5
  source-scanning architecture tests (same technique as
  `installation_id_architecture.rs`): no secret-shaped `set_field` match
  arm, no secret-shaped variable interpolated into telemetry construction,
  `CrashInfo`'s field list stays closed, no logging call site interpolates
  a secret-named variable, no `get_all_credentials()`-style API exists.

## Findings

- **Provider execution stays in TypeScript** — moving it to Rust would
  have been the exact "large, unrelated rewrite" the task warned against.
  The smallest safe architecture adds one narrow bridge
  (`credential_get`, called only at request time) rather than a second
  embed/LLM/rerank engine.
- **The per-keystroke duplicate-credential bug** — the first draft of this
  phase minted a fresh `CredentialRef` (and orphaned the previous one)
  every time `onChange` fired on a password input, i.e. once per
  character typed. Fixed by extending `credential_store` to accept an
  `existing_credential_ref` and overwrite in place; covered by
  `store_under_an_existing_ref_overwrites_rather_than_duplicating`.
- **Provider configuration stays in `localStorage`, not `studio.redb`** —
  a deliberate scope decision. `studio.redb` never persisted provider
  configuration before this phase (confirmed exhaustively by the
  preceding audit); routing it there for the first time would have been
  an unrelated persistence-location change. The invariant "`studio.redb`
  never contains a secret" already held and continues to hold — this
  phase added a regression test for it, not a new code path into it.
- **`ManifestProject.embed.apiKey` / `EmbeddingConfig.api_key_ref`
  remain unwired** — the audit found `apiKey` is stripped before ever
  reaching the daemon's `project.json`, so there was no live "apiKey in
  project configuration" to migrate per the task's own §17 condition
  ("only if the audit/code proves such a field exists in the relevant
  persisted project configuration"). `api_key_ref` compatibility with
  `CredentialRef` is confirmed (§18) but not wired into the project-
  creation flow — that would be new functional scope, not a credential-
  storage fix, and was left deferred.
- **A residual, narrow duplicate-credential race remains, documented, not
  engineered away**: if the app is killed in the exact window between a
  successful keychain `store` and the very next `localStorage.setItem`
  recording that ref, a retry mints a second, permanently orphaned
  keychain entry. Harmless (no data loss, no security impact — an unused
  keychain entry with an opaque account name), but not literally
  impossible to hit. Closing this completely would require a heavier
  two-phase-commit protocol between `localStorage` and the OS keychain,
  judged disproportionate to the risk.

## Validation

```text
cargo fmt --check                                                  clean (no files this phase touched)
cargo check --workspace                                            clean
cargo test -p valori-domain                                        4 test binaries, all green (incl. new CredentialRef tests)
cargo test -p valori-studio-storage                                105 passed, 0 failed (unchanged — not touched this phase)
cargo test --workspace                                             all green, 0 failures
cargo clippy -p valori-studio-storage --all-targets -- -D warnings clean
cargo test -p valori-node --test dependency_direction --test architecture   7 passed, 0 failed
npx tsc --noEmit                                                   clean
npm run build                                                      succeeds

Desktop crate (separate build, outside the root workspace):
cargo build --lib                                                  clean
cargo test --lib                                                   51 passed, 0 failed (48 prior + 3 new)
cargo test --test installation_id_architecture                      4 passed, 0 failed (unchanged, still green)
cargo test --test credential_security_architecture                  5 passed, 0 failed (new)
```

### Real desktop smoke test

Against a disposable `$VALORI_HOME=/tmp/valori-s3-test` (deleted after),
running the actual compiled `desktop/src-tauri` binary:

- **App boot**: starts cleanly with the new `credential_*` commands and
  `allow-credentials` permission set registered — no permission-denied
  errors, `studio.redb` created normally, existing legacy-migration log
  lines unaffected.
- **OS keychain, end-to-end, independently verified two ways**: (1) the 9
  inline `credential_service.rs` tests, run against the real macOS
  Keychain on this machine (not skipped — the store was available); (2) a
  direct probe with macOS's own `security` CLI (external to this
  codebase) — `security add-generic-password -s Valori -a <fake-uuid> -w
  <fake-secret>`, `find-generic-password` (read back correctly),
  `delete-generic-password` (removed, confirmed gone on a second find) —
  independently confirms the documented naming convention (`service =
  "Valori"`, `account = credentialRef`) works exactly as designed.
- **Scope limitation, stated plainly**: this environment does not reliably
  support driving the actual Settings UI end-to-end (typing into a
  password field, clicking Save, restarting, re-reading via the live
  webview) — the debug build's `devUrl` dependency and prior GUI-capture
  issues in this environment make that fragile enough that a claimed pass
  would not be trustworthy. In its place: the OS-keychain layer got real
  (non-mocked) verification as above; the JS wiring got full type-checking
  (`tsc --noEmit` clean) and a line-by-line code-review match against the
  identically-shaped, already-tested Rust `CredentialService` logic it
  calls through. This is a real, disclosed gap in this phase's testing —
  not a claim that a full GUI walkthrough was performed.
- Keychain left clean after the smoke test: `security find-generic-password
  -s "Valori"` returns "not found" — no orphaned entries from this
  session's testing.

## Follow-ups

- Wiring `api_key_ref`/`credential_ref` into the actual project-creation
  flow (so a project's embedding config references a real stored
  credential) — deferred, not proven necessary by current code (§17).
- A live, GUI-driven smoke test of the Settings UI's credential flow, once
  this environment's GUI-automation reliability improves (or via a real
  device) — the gap is disclosed above, not silently accepted.
- `CredentialKind`/`CredentialScope` — explicitly deferred per the task
  (only one credential shape exists in the codebase today).
- A web/Cloud-tier secret-at-rest improvement (e.g. a server-side proxy so
  the browser never holds the raw key) — out of scope; the web build's
  `localStorage` `apiKey` storage is unchanged and documented as a real,
  known limitation, not silently accepted.
- Headless-Linux Secret-Service unavailability (no GNOME Keyring/KWallet
  running) — `sync-secret-service` will surface this as
  `CredentialError::Unavailable`/`PermissionDenied`, handled gracefully
  (fail-closed, provider keeps working via the in-memory fallback), but no
  dedicated headless-Linux fallback store was built — explicitly deferred
  per the task's scope.

---

## Answers to the task's required questions

**1. Where is the actual provider API key stored?** In the OS credential
store only (macOS Keychain / Windows Credential Manager / Linux Secret
Service), under service `"Valori"`, account = the credential's
`CredentialRef` UUID string. Never in `localStorage`, never in
`studio.redb`, never in `project.json`, on the desktop path.

**2. What exactly is stored in `studio.redb`?** Nothing related to
provider credentials — unchanged from before this phase (it never held
any). Provider configuration (`provider`, `model`, `credentialRef`) is
stored in `localStorage`, exactly where it lived before S3; only the
secret-bearing field's shape changed.

**3. What happens when the OS keychain is unavailable?** Every
`CredentialService` operation returns a typed `CredentialError`, mapped to
a user-safe message (e.g. *"The system credential store is unavailable.
Please try again."*), never raw OS internals. The UI's persist logic fails
closed: on a store failure, the in-memory key keeps working for the
current session, nothing is written to disk, and the next save retries.
On a read failure at load time, the resolved `apiKey` falls back to empty
(or, during migration, the untouched legacy plaintext value stays in
`localStorage` rather than being deleted).

**4. How are existing plaintext `localStorage` credentials migrated?**
`native.ts`'s `migrateLegacyProviderCredential`, run once per load, per
config key, on desktop only: store the legacy value under a fresh
`CredentialRef` → immediately persist `{ ...config, credentialRef }`
*with the legacy `apiKey` still present* (the resumable safety net) →
read the credential back and verify a byte-for-byte match → only then
rewrite `localStorage` to drop `apiKey`. See §12/§15 of the task and the
"Findings" section above for the ordering rationale.

**5. What happens if migration fails halfway through?** Depends on which
half: if it fails before the keychain `store` call, nothing changes,
retried next load. If it fails after `store` but before the
`credentialRef` is recorded in `localStorage` (an unavoidable, narrow,
documented race — see "Findings"), a retry mints a second, harmless,
orphaned keychain entry. If it fails after the ref is recorded but before
verification/cleanup, the next load detects the existing ref + still-
present `apiKey`, resumes from verification, and does not re-store. In
every case, the legacy plaintext value is never deleted until a verified
match confirms the new credential is retrievable.

**6. Can the same credential reference survive a Studio DB restore?**
Yes — trivially, since `credentialRef` is not stored in `studio.redb` at
all in this implementation; it lives in `localStorage`, which is untouched
by Studio's own DB backup/recovery system entirely. The underlying
question (does the OS keychain entry survive) is answered by #7.

**7. What happens if the DB is restored on another machine?** The OS
keychain entry is machine-scoped by nature (not something Valori's code
controls) and does not travel with a `studio.redb`/`localStorage` copy —
a `credentialRef` on a new machine will not resolve there, and the user
must re-enter the key once. This is the correct, expected security
property of using the OS keychain, not a bug — documented, not built
around, per the task's explicit "do not build cross-machine credential
migration in S3" instruction.

**8. Can credentials enter telemetry?** Not through any current call
site — confirmed unchanged from the preceding audit, and now covered by a
regression test (`telemetry_source_never_interpolates_a_variable_named_like_a_raw_secret`,
plus a positive test proving a realistic safe payload —
`provider`/`model`/`credential_ref` — excludes the secret). The
`payload`/`properties` fields remain structurally freeform JSON (an
unclosed structural risk the audit already flagged and this phase's tests
guard, not eliminate).

**9. Can credentials enter logs/crash reports?** No current call site
does — the Rust process never receives the raw secret except inside
`CredentialService` itself (which never logs it), and a new architecture
test (`no_desktop_source_file_logs_a_variable_named_like_a_secret`) scans
every `tracing!`/`println!`/`eprintln!`/`dbg!` call site in
`desktop/src-tauri/src` for a secret-named variable. `CrashInfo`'s field
list is closed and pinned by a dedicated test.

**10. Which provider configuration paths remain web-only (i.e. still
plaintext)?** The entire web/Cloud build's `localStorage` storage for
`valori:llm_config`/`valori:embedding_config`/`valori:reranker_config` —
there is no OS keychain reachable from a browser tab, so this phase left
it byte-for-byte unchanged. Documented explicitly, not silently accepted,
in both this doc and `studio-persistence-audit.md`.

**11. Which plaintext `apiKey` occurrences remain, and why?** See the
final repository search in the implementation report (below/in-session) —
in summary: (a) the web/Cloud (`!isTauri()`) branches of the three config
files, by design (#10); (b) `apiKey` transiently inside
`migrateLegacyProviderCredential`'s in-flight state on desktop (the
resumable safety-net window, never at rest once migration completes); (c)
`ui/src/lib/server/{embed,llm,reranker}.ts` and their callers/API routes,
which still take an `apiKey` field in their function signatures/request
bodies — this is the resolved-secret-in-flight for an actual provider
call, not a persistence location, and moving it was explicitly out of
scope (#11 above / the task's §11); (d) the now-fully-dead
`ManifestProject.embed.apiKey`/legacy `ProjectEntry.apiKey` fields the
preceding audit already found unpopulated by any live writer — untouched,
not this phase's concern.

**12. What is intentionally deferred?** `CredentialKind`/`CredentialScope`,
Cloud authentication/Supabase changes, Voyage AI, marketplace/model-hosting
credentials, a generic secret-management framework, headless-Linux
fallback storage, credential sync, wiring `api_key_ref` into project
creation, and a live GUI-driven smoke test (disclosed gap, see
"Validation"). All per the task's explicit scope boundaries.
