# Valori Studio — S3 Credential Security Audit

**Status:** Read-only investigation. No source code, dependencies, `studio.redb`,
`localStorage`, or project manifests were modified as part of this audit.
**Scope:** repository-wide search + targeted inspection of `ui/src`,
`desktop/src-tauri/src`, `crates/valori-domain`, `crates/valori-studio-storage`,
`crates/valori-models`, `crates/valori-daemon`, `crates/valori-node`.

---

## 1. Executive summary

Provider API keys (OpenAI, Cohere, Groq, Together AI, and any "custom"
OpenAI-compatible endpoint) are entered by the user in Valori Studio's
Settings/onboarding UI and stored **in plaintext in browser `localStorage`**
under three keys: `valori:llm_config`, `valori:embedding_config`,
`valori:reranker_config`. This is the confirmed highest-severity issue the
prior persistence audit flagged, and this audit traces it precisely.

The good news, established with concrete evidence below: **the secret never
crosses into the Rust/Tauri layer today.** The entire life of a provider API
key — entry, storage, and use — happens inside the Next.js/React process
(browser webview + the bundled local Next.js server). No Tauri command
accepts an API key as an argument, `studio.redb`'s `preferences` table has no
field or match arm capable of holding one, `StudioTelemetryEvent`/
`TelemetryEnvelope` have no such field and no current call site populates
one, and `CrashInfo` has no such field. This materially narrows the S3 fix:
the leak is real and severity-appropriate to fix, but it is contained to one
layer, not smeared across the whole persistence stack.

A separate, already-shipped piece of prior art directly previews the correct
design: `crates/valori-daemon/src/project.rs`'s `EmbeddingConfig` already
uses `api_key_ref: Option<String>` (a reference, e.g. `"env:OPENAI_API_KEY"`)
— its own doc comment explicitly contrasts this with "`ui/`'s current
`ProjectEntry.embed.apiKey`" — and `crates/valori-domain/src/project.rs`
explicitly *deferred* embedding config from the canonical `Project` type for
exactly this reason ("A shared model that can hold a secret needs a secrets
decision first"). This audit is that decision.

Valori Cloud authentication (Supabase session, Cloud API keys, personal
access tokens) is architecturally separate already — different storage
mechanism entirely (server-side Supabase RPC + hashed storage + HttpOnly
cookie session, not `localStorage`) — confirmed below in §11. The two must
not be conflated in the S3 design.

---

## 2. Current credential architecture

```text
Browser / Tauri webview (Next.js client components)
      │
      │  apiKey entered in a settings form
      ▼
React state (useState)
      │
      │  every state change
      ▼
localStorage["valori:llm_config" | "valori:embedding_config" | "valori:reranker_config"]
      │
      │  plain JSON.stringify, no encryption, no Tauri bridge involved
      ▼
(persists indefinitely — read back on every mount via localStorage.getItem)
      │
      │  when a feature actually needs to call the provider, the FULL config
      │  object (including apiKey) is sent as an HTTP POST body from the
      │  client to a same-origin Next.js API route
      ▼
Next.js API route (server-side: /api/embed-query, /api/why, /api/ingest, ...)
      │
      │  cfg.apiKey read directly out of the POST body
      ▼
ui/src/lib/server/{embed,llm,reranker}.ts
      │
      │  Authorization: `Bearer ${cfg.apiKey}` header built in-process
      ▼
Outbound HTTPS request to the actual provider (OpenAI / Cohere / Groq / etc.)
```

**Nothing in this chain touches Rust.** `desktop/src-tauri` never sees the
key — confirmed by an exhaustive `#[tauri::command]` argument-name search
(§5) and by the fact that `native.ts`'s generic `setPreference`/
`getPreference` bridge (the only path from JS into `studio.redb`) is never
called with any embed/llm/reranker/apiKey-shaped key (§8).

This is a browser-only vulnerability today, not a `studio.redb` vulnerability
— but it is still a real one: `localStorage` for a Tauri app is a plain
SQLite/LevelDB file on disk in the app's webview data directory, unencrypted,
readable by anything with filesystem access to that user account, and
(unlike `studio.redb`) has **no** existing recovery/corruption/backup
discipline applied to it at all.

---

## 3. Provider inventory

Only providers actually present in the repository, with citations:

| Provider | Purpose | Secret required? | Where configured? | Where persisted? | Who consumes it? |
|---|---|---|---|---|---|
| **OpenAI** | LLM (`llm.ts`) + Embeddings (`embed.ts`) | Yes | `useLLMConfig.ts`, `useEmbeddingConfig.ts`, Settings UI | `localStorage["valori:llm_config"]` / `["valori:embedding_config"]` | `ui/src/lib/server/llm.ts`, `ui/src/lib/server/embed.ts` (`Authorization: Bearer`) |
| **Ollama** | LLM + Embeddings, local | No (`note: "no API key"`, [useLLMConfig.ts:19](../../ui/src/lib/hooks/useLLMConfig.ts:19)) | same hooks | same `localStorage` keys (empty `apiKey`) | `embed.ts`/`llm.ts`, no `Authorization` header sent |
| **Groq** | LLM only | Yes (OpenAI-compatible) | `useLLMConfig.ts` | `localStorage["valori:llm_config"]` | `ui/src/lib/server/llm.ts` |
| **Together AI** | LLM only | Yes (OpenAI-compatible) | `useLLMConfig.ts` | `localStorage["valori:llm_config"]` | `ui/src/lib/server/llm.ts` |
| **Cohere** | Embeddings ([useEmbeddingConfig.ts:18](../../ui/src/lib/hooks/useEmbeddingConfig.ts:18)) + Reranker ([SettingsModal.tsx:196](../../ui/src/components/settings/SettingsModal.tsx:196)) | Yes | `useEmbeddingConfig.ts` (embed) / `SettingsModal.tsx` (rerank) | `localStorage["valori:embedding_config"]` / `["valori:reranker_config"]` | `embed.ts` / `reranker.ts` |
| **Custom** (any OpenAI-compatible endpoint) | LLM, Embeddings, Reranker | Optional (`apiKey?`) | all three hooks/UI | all three `localStorage` keys | `embed.ts`/`llm.ts`/`reranker.ts` |
| **Voyage AI** | Embeddings — **Rust-side only** | Yes | Not exposed in `ui/`'s provider selector at all | N/A (no UI path found) | `crates/valori-models/src/provider/voyage.rs`; a caller supplies `api_key: impl Into<String>` — no repository call site was found wiring a live secret into it from the standalone node's config today (`VALORI_EMBED_PROVIDER` in `config.rs` documents `ollama`/`openai`/`custom` only). **UNKNOWN — requires implementation/runtime verification** whether Voyage is reachable via any currently-wired code path. |
| **HuggingFace** | Local model download only (`transformers.js`) | **No** — [`transformers.ts:4`](../../ui/src/lib/embeddings/transformers.ts) references the HF Hub only as a public model-weights source for in-browser/WASM embedding, not an authenticated API. Not a credential-bearing provider. | — | — | — |
| **Anthropic / Gemini** | **Not present anywhere in the repository.** No provider entry, no hook, no Rust client. Not included per the "do not invent providers" instruction. | — | — | — | — |

`VALORI_EMBED_API_KEY` (standalone `valori-node`, [`config.rs:145`](../../crates/valori-node/src/config.rs:145)) is a **separate, server-operator-set environment variable** for the node's own ingest pipeline — not user-entered in the Studio UI, not persisted anywhere Studio owns, out of scope for the browser-`localStorage` problem this audit addresses. Mentioned for completeness of "every place a provider secret exists in this codebase."

---

## 4. Complete secret data-flow map

Traced against actual code, not assumed:

```text
User types API key into a Settings input (type="password")
        │
        ▼
onChange → setConfig({ apiKey: value })              (React state, in-memory)
        │
        ▼
useEffect(() => localStorage.setItem(KEY, JSON.stringify(config)), [config])
        │   [useLLMConfig.ts:68-74], [useEmbeddingConfig.ts:69-75],
        │   [SettingsModal.tsx:246] (reranker, synchronous, not an effect)
        ▼
localStorage — plaintext JSON, keyed by
  "valori:llm_config" | "valori:embedding_config" | "valori:reranker_config"
        │
        │  (persists until the user clears it or clears app data)
        ▼
Some UI action needs to actually call the provider (e.g. "Ask" in AskTab.tsx,
document ingest, embedding a search query) →
fetch("/api/embed-query" | "/api/why" | "/api/ingest", { body: JSON.stringify(config) })
        │   [embed-query/route.ts:6], config read straight out of req.json()
        ▼
Next.js API route (server-side, same process as the bundled ui-server)
        │
        ▼
embedTexts()/callLLM()/rerank() in ui/src/lib/server/{embed,llm,reranker}.ts
        │
        │  headers: { Authorization: `Bearer ${cfg.apiKey}` }
        │   [embed.ts:33,51,115], [llm.ts:74], [reranker.ts:28,41]
        ▼
Outbound fetch() to the provider's real HTTPS endpoint
```

No step in this trace passes through `native.ts`'s Tauri bridge, no step
invokes any `#[tauri::command]`, and no step writes to `studio.redb`. This
flow is identical whether Studio is running as the desktop app (Tauri
webview + bundled local Next.js server) or as the Cloud web app (browser +
Vercel/hosted Next.js) — see §6 for why that distinction still matters.

**One structurally-present but dead field**: `ManifestProject.embed?.apiKey`
([`useProjectManifest.ts:28`](../../ui/src/lib/hooks/useProjectManifest.ts:28))
and the legacy `ProjectEntry.apiKey?: string`
([`projects.ts:58`](../../ui/src/lib/server/projects.ts:58)) both declare a
field capable of holding an API key on the *project manifest* type — but:
- `POST /api/projects` explicitly **strips** `apiKey` before forwarding to
  the daemon ([`route.ts:119-121`](../../ui/src/app/api/projects/route.ts:119)
  — only `provider`, `model`, `endpoint` are forwarded).
- `GET /api/projects`'s response is built by `toManifestShape()`
  ([`project-adapter.ts:56-57`](../../ui/src/lib/server/project-adapter.ts:56)),
  which reads from the daemon's `DaemonEmbeddingConfig`
  ([`daemon.ts:99-104`](../../ui/src/lib/server/daemon.ts:99)) — that type
  has `api_key_ref?: string`, never `apiKey`. `embed.apiKey` is therefore
  **never populated** by any live code path.
- A repository-wide search for `.embed.apiKey`/`embed?.apiKey` being *read*
  anywhere in `ui/src` returns zero results. The field exists in the
  TypeScript type but no code writes or reads a real value through it.
- The legacy `createProject()` in `projects.ts` (the one function that would
  accept and write a caller-supplied `apiKey` to `~/.valori/projects.json`
  directly) is **not called by any route** — `route.ts` calls
  `daemon.createProject()` instead. It is dead code for the live project-
  creation flow.
- **One residual risk found**: `touchProject()` (called on project
  open/close — [`open/route.ts:3`](../../ui/src/app/api/projects/[name]/open/route.ts:3),
  [`close/route.ts:3`](../../ui/src/app/api/projects/[name]/close/route.ts:3))
  performs a read-modify-write of the *entire* legacy manifest list via
  `readManifest()`/`writeManifest()` ([`projects.ts:357-362`](../../ui/src/lib/server/projects.ts:357)).
  If a legacy `~/.valori/projects.json` entry from a version of Studio old
  enough to have used the removed `apiKey`-writing path still has that field
  set, this read-modify-write would **round-trip and re-persist it**
  unchanged (not introduce a new one, but not strip an old one either).
  **UNKNOWN — requires runtime verification** against a real legacy file;
  no fixture or test in the repository exercises this exact scenario.

---

## 5. Persistence audit

| Location | Read? | Written? | Secret stored? | Reference stored? | Production caller? |
|---|---|---|---|---|---|
| `localStorage["valori:llm_config"]` | Yes | Yes | **Yes, plaintext** | No | `useLLMConfig.ts` |
| `localStorage["valori:embedding_config"]` | Yes | Yes | **Yes, plaintext** | No | `useEmbeddingConfig.ts` |
| `localStorage["valori:reranker_config"]` | Yes | Yes | **Yes, plaintext** | No | `SettingsModal.tsx` |
| `localStorage["valori:projects-list"]` (SWR cache) | Yes | Yes | No (see §4 — `embed.apiKey` never populated) | No | `useProjectManifest.ts` |
| `sessionStorage` | — | — | Not used anywhere in `ui/src` (grep confirmed zero hits) | — | — |
| `document.cookie` | Yes | Yes | No — Supabase **session token**, not a provider secret (§11) | — | `utils/supabase/client.ts:16,26,33` |
| `studio.redb` (`preferences` table) | — | — | **No** — no field exists for it; `set_field`'s match arms are an exhaustive allowlist (theme, language, accentColor, onboardingVersion, telemetryConsent, windowState, lastPage, installationId, workspaceDir, modelDir, dockIcon, termsAccepted) with a silent no-op default arm; no embed/llm/reranker/apiKey key is in that list | N/A yet | N/A |
| `project.json` (daemon-managed, current path) | Yes | Yes | **No** — `EmbeddingConfig.api_key_ref: Option<String>` only ([`project.rs:67`](../../crates/valori-daemon/src/project.rs:67)) | **Yes**, by design (currently unpopulated — nothing writes a ref yet either; the field is schema-complete, not behavior-complete) | `valori-daemon` |
| `~/.valori/projects.json` (legacy `ProjectEntry`, `projects.ts`) | Yes (`touchProject` only) | Yes (`touchProject` only) | Potentially, for pre-migration entries only (§4's residual-risk finding) | No | `open`/`close` routes, indirectly |
| `preferences.json` (pre-`studio.redb` legacy) | Only via S2a migration engine | Never (migration is read-only against legacy files) | **UNKNOWN** — no `apiKey`/embed field was ever part of `LegacyPreferences`'s schema (checked `migration.rs`); a hand-edited or third-party-modified legacy file could theoretically contain arbitrary JSON, but the migration parser only extracts the named fields it knows about, so nothing extra would propagate into `studio.redb` | — | S2a migration (read-only) |
| Environment variables | Yes (`VALORI_EMBED_API_KEY`, server-side node only) | N/A (operator-set) | Yes, but operator-controlled infra config, not user-entered Studio UI secret | N/A | `valori-node` |
| Command arguments | — | — | Not found — no CLI flag or spawned-process argument carries a provider key anywhere in `desktop/src-tauri` (checked `daemon_manager.rs`'s env-passing code; it passes `VALORI_HOME` only) | — | — |
| URL query parameters | — | — | Not found — every provider-config transfer uses a POST body, never a query string (confirmed for `embed-query`, `why`, `ingest` routes) | — | — |
| URL fragments | — | — | Not found | — | — |
| Cookies (non-Supabase) | — | — | Not found | — | — |
| Cloud database (Supabase) | Yes | Yes | No provider secrets — only Cloud's own API keys, hashed (§11) | N/A | Cloud settings pages |
| `telemetry_queue` (`studio.redb`) | — | — | **No** — see §6 | — | — |
| Crash markers | — | — | **No** — see §7 | — | — |

---

## 6. Studio.redb and telemetry audit

**`StudioPreferences`** ([`preferences.rs`](../../crates/valori-studio-storage/src/preferences.rs)):
fields are `theme`, `language`, `accent_color`, `onboarding_version`,
`telemetry_consent`, `window_state`, `last_page`, `installation_id`,
`workspace_dir`, `model_dir`, `dock_icon`, `terms_accepted`. No field is
named or shaped like a provider credential. The generic `set_field`/
`get_field` accessors ([`preferences_service.rs:87-132`](../../desktop/src-tauri/src/preferences_service.rs:87))
are an explicit `match` over a fixed key list with a silent-ignore default —
a caller cannot smuggle an arbitrary key like `"llmApiKey"` into `studio.redb`
through the generic bridge even if `ui/`'s JS code tried to; it would just be
dropped (`debug!("Ignoring unknown or unmodeled preference key...")`).

**`projects` / `project_cache` tables**: hold `StudioProjectRecord` (name,
paths, favorite/recent metadata) — no embedding/LLM config field. Confirmed
via `grep` across `crates/valori-studio-storage/src`.

**`sessions` table**: `StudioSessionRecord` (id, `installation_id`,
app_version, platform, timestamps, crashed) — no secret-shaped field.

**`telemetry_queue` table / `StudioTelemetryEvent`**
([`telemetry.rs:80-95`](../../crates/valori-studio-storage/src/telemetry.rs:80)):
`event_id`, `created_at`, `event_name`, `session_id`, `payload:
serde_json::Value` (freeform), `attempt_count`, `last_attempt_at`,
`category`. **The `payload` field is structurally freeform JSON** — nothing
in the type system prevents a future call site from passing provider config
into it. Traced every current caller of the telemetry `send()`/`report*()`
functions ([`telemetry.ts`](../../ui/src/lib/telemetry.ts),
`AppShellGate.tsx:102,144,197`, `startupMarks.ts:63`): every one passes only
timing marks (numbers) or nothing at all. **No current call site puts
provider configuration into telemetry.** This is a structural risk to guard
against going forward (e.g. with a lint/test), not an active leak today.

**`TelemetryEnvelope`** (Rust wire struct,
[`telemetry.rs:112-124`](../../desktop/src-tauri/src/telemetry.rs:112)):
fixed fields (`schema`, `source`, `event_id`, `timestamp`, `session_id`,
`installation_id`, `version`, `platform`, `arch`, `event`, `properties`) —
`properties` is the same freeform `serde_json::Value` passed through from
the queued event's `payload`, so the same conclusion applies: structurally
possible, not currently exercised.

**Answer to the desired-future-invariant question**: `studio.redb` does not
currently contain — and has no live code path capable of accidentally
receiving — a provider credential, reference or otherwise. The
`api_key_ref` concept (§3) exists today only in the daemon's separate
`project.json`, not in `studio.redb`.

---

## 7. Logging and crash-report audit

**Rust logging**: searched every `println!`/`dbg!`/`tracing::{debug,info,
warn,error}!` call in `desktop/src-tauri/src/*.rs` for proximity to
key/token/secret/password/auth/config/embed/llm terms — **zero matches**.
The Rust layer never logs anything related to provider config, which
follows directly from §4/§6: it never receives the data to log in the first
place.

**JS/TS logging**: searched every `console.{log,error,debug,warn}` call in
`ui/src` for the same terms. The only matches
([`cloud/settings/api-keys/actions.ts:40,85,122,148`](../../ui/src/app/cloud/settings/api-keys/actions.ts),
[`developer/actions.ts:38,79`](../../ui/src/app/cloud/settings/developer/actions.ts))
log only `auditError.message` (Supabase's own audit-log RPC failure text)
alongside a hardcoded event-type string like `'api_key.created'` — never a
key value, never the RPC arguments. These are all in the **Cloud** settings
pages (§11), unrelated to local provider secrets.

**Error-path check** (the instruction to not assume error paths are safe
just because success paths are): `embed.ts`, `llm.ts`, `reranker.ts` build
error messages exclusively from `res.status` or the *provider's own*
`error.message` field in its JSON error body (e.g.
`` `OpenAI: ${e.error?.message ?? res.status}` `` — [`embed.ts:39`](../../ui/src/lib/server/embed.ts:39)).
No code path serializes the request config (`cfg`) or its `apiKey` field
into an error message, response, or thrown `Error`. The Next.js route
handlers' own `catch` blocks (`embed-query/route.ts:11`, `errorResponse()`
in `http.ts:25-31`) similarly surface only `err.message`, never the request
body. **One residual, low-severity, not-fully-closable risk**: the
`"custom"` provider is a user-supplied, non-Valori-controlled endpoint —
Valori's code trusts whatever JSON that endpoint returns for its `error`
field, so a malicious or misbehaving custom endpoint *could* choose to echo
request data back in its own error response, which would then surface in
the Studio error toast. This is a property of "the user pointed Studio at
an untrusted endpoint," not a Valori code defect, but worth naming.

**Crash reporting** (`CrashInfo`,
[`telemetry.rs:401-411`](../../desktop/src-tauri/src/telemetry.rs:401)):
fields are `panic_hash` (a hash, not the raw message —
`panic_hash_is_a_fixed_width_hex_digest_not_the_raw_message` test already
enforces this), `panic_location` (file:line, not the panic payload text
itself in the marker — though the *source* panic message is hashed, not
stored, by design), `previous_session`, `uptime_before_crash_secs`. No field
for provider config. Since the Rust process never receives a provider
secret (§4), a Rust panic categorically cannot capture one in its payload.
**Cannot make the same claim about the JS side**: if a JS-side error
(unrelated to a real crash) were ever wired into a future "send JS error to
Rust" bridge, and that error's message/stack happened to embed a raw
`apiKey` (e.g. from a thrown `new Error(JSON.stringify(cfg))` — not found
anywhere today, but not structurally impossible to introduce later), it
would flow through the same `panic_location`/`panic_hash` capture. Today: no
such bridge exists (`check_and_clear_crash_marker` only reads a Rust-written
marker file), so this is **UNKNOWN — requires implementation/runtime
verification** only in the sense of "verify this stays true," not an active
gap.

---

## 8. Desktop vs Web classification

| Credential | Desktop-only | Web-only | Shared | Cloud-only |
|---|---|---|---|---|
| OpenAI/Cohere/Groq/Together/Custom provider API keys | — | — | **Shared** — identical code path (`useLLMConfig.ts` etc.) runs in both the Tauri webview and the hosted Cloud web app; `localStorage` is per-browser-profile/per-webview-instance in both cases, so the *value* is not shared across them, but the *mechanism* is | — |
| `installation_id` | Yes (Studio-specific identity, per the prior phase) | — | — | — |
| Supabase session / Cloud API keys / personal access tokens | — | — | — | **Cloud-only** — server-side Supabase RPC + cookie session; not reachable from, or relevant to, the Tauri/Rust layer at all |
| `VALORI_EMBED_API_KEY` | — | — | — | Server-operator config for a standalone/self-hosted `valori-node`, not a Studio end-user credential in either surface |

**Consequence for the S3 design**: an OS-keychain-backed `CredentialService`
is meaningful **only for the desktop (Tauri) build** — there is no OS
keychain to call into from a browser tab running the Cloud web app. The
design must keep the existing `localStorage` fallback for the web/Cloud
surface (matching the precedent already established for `installation_id`
and `theme` — see `native.ts`'s `isTauri()` branch pattern) and must not
"blindly remove `localStorage`" per the task's explicit instruction: doing
so would break the web app, which has no alternative today.

---

## 9. Cloud boundary

Confirmed genuinely separate mechanisms, not mixed:

- **Local provider credentials** (OpenAI/Cohere/Groq/Together/custom API
  keys): client-side `localStorage`, plaintext, sent to a same-origin
  Next.js API route on demand, forwarded to the provider. No hashing, no
  server-side storage of any kind.
- **Valori Cloud authentication**: Supabase-managed. `createApiKey()`
  ([`api-keys/actions.ts:7-42`](../../ui/src/app/cloud/settings/api-keys/actions.ts:7))
  calls a `create_api_key` Postgres RPC; the comment at line 40-41 states
  explicitly: *"`plaintext_key` is returned exactly this once — the RPC
  never stores it, only a hash."* Session identity is a cookie-based
  Supabase SSR session (`utils/supabase/client.ts`), not `localStorage`.
  Personal access tokens and service-account tokens
  ([`developer/actions.ts`](../../ui/src/app/cloud/settings/developer/actions.ts))
  follow the identical reveal-once/hash-stored pattern.

These two systems do not currently touch each other's storage. The S3 design
should preserve this separation explicitly — `CredentialService` (§14-15) is
scoped to **local provider credentials only**; Cloud auth already has its own
correctly-designed (hash-stored, reveal-once) system and needs no S3 change.

---

## 10. OS keychain research

**Existing dependencies checked first** (`Cargo.lock` at both the workspace
root and `desktop/src-tauri/Cargo.lock`): no `keyring`, `security-framework`
*as a direct dependency* (it's present only transitively, pulled in by the
TLS stack for certificate handling — not usable as a keychain API without
adding it directly and writing FFI-level code against it), no
`secret-service`, no `keytar`, no Tauri Stronghold or keyring plugin. **No
suitable dependency exists today** — any implementation requires a new
dependency, which this audit is explicitly not authorized to add.

**Recommended candidate for the implementation phase**: the
[`keyring`](https://crates.io/crates/keyring) crate.

| Criterion | Assessment |
|---|---|
| Crate | `keyring` (not `keyring-rs`'s older name, same project) |
| Maintenance | Actively maintained, part of the `hwchen`/community-governed `keyring-rs` project; widely used (millions of downloads), used by other Tauri-ecosystem projects |
| Platform support | macOS (Keychain Services via `security-framework`, already transitively in the dependency tree — a nice synergy), Windows (Windows Credential Manager), Linux (Secret Service via D-Bus, `libsecret`) — all three platforms this project targets ([`Cargo.toml`](../../desktop/src-tauri/Cargo.toml)'s macOS-specific deps confirm macOS is a first-class target; Windows/Linux Tauri targets are implied by the general Tauri setup but not verified in this pass — **UNKNOWN, requires checking CI/build matrix**) |
| API | Synchronous, blocking (`Entry::new(service, user).set_password()/get_password()/delete_credential()`) |
| Sync/async behavior | **Blocking** — must be called from a `tokio::task::spawn_blocking` or similar inside any async Tauri command, the same pattern this codebase already uses for `tracing::info_span`/blocking redb calls (`StudioDatabase`'s own concurrency model, §7 of `studio-storage.md`, already establishes "no `Arc<Mutex<_>>`, every call opens its own transaction" — a `CredentialService` should follow an analogous "each call is a synchronous OS call, wrapped where needed" pattern rather than trying to make it look async) |
| Tauri compatibility | Confirmed compatible in general Tauri-ecosystem use (multiple published Tauri apps use `keyring` directly, no known plugin conflicts) — **not verified by actually adding and building it in this repo**, since this audit is read-only. **UNKNOWN — requires implementation-phase verification** |
| License | MIT OR Apache-2.0 — matches this repository's own dual-license (`Cargo.toml` headers throughout use `MIT OR Apache-2.0`), no license friction |
| Security model | Delegates entirely to the OS's own credential store — Valori writes no encryption code itself. macOS Keychain and Windows Credential Manager are both encrypted at rest and gated by the OS's own access-control (user login, optionally biometric unlock). Linux's Secret Service backend varies by desktop environment (GNOME Keyring, KWallet) and **may not be present on minimal/headless Linux installs** — this is a real gap the design must account for (fallback behavior needed, not a `keyring` crate flaw) |

**Alternative considered and rejected for now**: Tauri's own `stronghold`
plugin (`tauri-plugin-stronghold`) — a full encrypted-vault solution with
its own file format, useful for scenarios needing structured secret
*collections*, but heavier than what's needed here (one secret per provider
config), not OS-native (doesn't integrate with the user's actual system
keychain UI, e.g. macOS's Keychain Access app), and not currently a
dependency. `keyring` is a better fit because it maps 1:1 onto "one secret,
one OS-native entry" and requires no new file format or vault management.

---

## 11. Recommended target architecture (validated against existing code, not implemented)

The proposed shape from the task prompt is directly compatible with what
already exists:

```text
Provider Config (client-side, ui/)
        │
   ┌────┼────────────┐
   │    │             │
provider  model   credential_ref   ← replaces today's `apiKey` field
                       │
                       ▼
              Tauri command (new)
                       │
                       ▼
              CredentialService (Rust)
                       │
                       ▼
              OS Credential Store (via `keyring`)
```

This slots directly on top of the existing `api_key_ref` precedent in
`crates/valori-daemon/src/project.rs`'s `EmbeddingConfig` — that field
already exists, is already documented as "a reference (env var name,
keychain entry id, etc.), never the raw secret," and is simply not populated
by any writer yet. S3 would be the phase that starts populating it.

`studio.redb`'s `StudioPreferences` would gain **no new secret-shaped
field** under this design — it already can't hold one (§6) — but if
per-provider config (not just per-project) needs to persist across
sessions, a `credential_ref`-only structure could live there following the
exact same pattern `installation_id` and `theme` already use (typed field,
`get_or_init`-style access, S1-era schema).

---

## 12. `CredentialRef` recommendation

**Proposed shape**: an opaque, stable string identifier — **not a raw
UUID exposed as "the credential"**, but a stable reference Valori mints and
the OS keychain stores under. Concretely:

```text
cred_<uuid-v4>          e.g. cred_01J8ZQK3R8YFPX5N7WACJH2VXM  (ULID-style, sortable)
   or
cred_<blake3-short>     content-addressed by (provider, created_at) — less useful
                        here since the *value* being hashed is exactly the
                        secret we're trying to keep out of any hash-derivable
                        form; a random UUID is simpler and equally opaque
```

Recommend a plain **UUID v4**, mirroring `InstallationId`/`SessionId`/
`ProjectId`'s existing pattern (`valori_domain::uuid_id!` macro) rather than
inventing a new ID scheme. Consistency with the rest of `valori-domain`
outweighs any marginal benefit of a slug or ULID.

- **Where it should live**: `crates/valori-domain`, as a new ID type
  (`CredentialRef` or `CredentialId`) via the existing `uuid_id!` macro —
  same crate, same pattern as `InstallationId`. This is consistent with
  `valori-domain`'s stated purpose ("cross-boundary platform vocabulary")
  and avoids a new crate (§13's dependency-implications question, answered
  below).
- **Serialization format**: same as every other `valori-domain` ID —
  `#[serde(transparent)]` string form (`"cred_..."` or plain UUID string,
  to be decided at implementation time; existing IDs like `InstallationId`
  serialize as bare UUID strings with no prefix, so plain-UUID is the more
  consistent choice unless a prefix is judged worth the inconsistency for
  human debuggability).
- **Does it belong in `valori-domain`?** Yes — it's cross-boundary
  vocabulary (used by `desktop/src-tauri`, potentially `valori-daemon`'s
  `EmbeddingConfig.api_key_ref` if that field is upgraded from
  `Option<String>` to `Option<CredentialRef>` at implementation time), the
  same justification `InstallationId`/`ProjectId`/`SessionId` already have.
- **Does it need to be persisted in project metadata?** Only the *reference*
  — `EmbeddingConfig.api_key_ref` already has the slot; whether it stays
  `Option<String>` or becomes `Option<CredentialRef>` is an implementation
  decision, not an architectural one (both are equally safe — the string is
  never the secret either way).
- **Do references need scopes?** Not yet — the current architecture is
  "one provider config per feature (embed/llm/reranker) per project," which
  doesn't require a scope concept beyond what `(provider, purpose)` already
  disambiguates in the OS keychain's own service-name field (e.g. service =
  `"valori-studio"`, account = `"{provider}:{purpose}:{project_id or global}"`).
  Introducing `CredentialScope` now would be speculative — defer until a
  real multi-tenant or multi-project-sharing-one-credential need appears
  (§16's "avoid speculative abstractions" instruction applies directly
  here).

---

## 13. `CredentialService` proposal (design only)

**Location**: `desktop/src-tauri`, not a new crate, not `valori-domain`, not
`valori-studio-storage`.

Reasoning:
- `valori-domain` is `std`-only cross-boundary *vocabulary* (types), not a
  place for I/O or OS integration — it has no existing precedent for
  wrapping an external system call, and adding one would violate its own
  stated scope.
- `valori-studio-storage` owns `studio.redb` specifically — a `redb`-backed
  crate. An OS-keychain wrapper has nothing to do with `redb` and doesn't
  belong there; forcing it in would blur that crate's single responsibility
  (established repeatedly across the S1-S2c/DR phases as "one file, one
  owner, one purpose").
- `desktop/src-tauri` already hosts every other OS-integration concern this
  app has (`daemon_manager.rs` for process supervision, `ui_server_manager.rs`
  for the bundled server, `telemetry.rs` for HTTP) — `CredentialService`
  belongs alongside `StudioPreferencesService`/`SessionService` as another
  typed service in the same crate, following the exact same
  `#[derive(Clone)] struct XService { ... }` + `#[tauri::command]` wrapper
  pattern already established by every existing service in this codebase.
- A new crate is unwarranted: `keyring`'s API surface is small (get/set/
  delete), there's no cross-boundary reuse need (only the desktop app calls
  it — the Cloud web app has no keychain to call), and creating a crate for
  a ~50-line wrapper would be the "abstraction for single-use code" the
  project's own coding guidelines warn against.

**Proposed operations** (design only, not implemented):

```rust
pub struct CredentialService;  // or holds config (service-name prefix, etc.)

impl CredentialService {
    pub fn create(&self, cred_ref: &CredentialRef, secret: &str) -> Result<(), CredentialError>;
    pub fn get(&self, cred_ref: &CredentialRef) -> Result<Option<String>, CredentialError>;
    pub fn exists(&self, cred_ref: &CredentialRef) -> Result<bool, CredentialError>;
    pub fn delete(&self, cred_ref: &CredentialRef) -> Result<(), CredentialError>;
}
```

**Dependency implications**: adds exactly one new dependency (`keyring`) to
`desktop/src-tauri/Cargo.toml` only — it must **not** be added to any
`no_std`/determinism-critical crate (`valori-kernel`, `valori-core`,
`valori-domain` itself must stay dependency-light per its own "minimal
dependencies" mandate). `CredentialRef` (the *type*) goes in
`valori-domain`; `CredentialService` (the *implementation*, the thing that
actually calls `keyring`) stays in `desktop/src-tauri`, exactly mirroring
how `InstallationId` (type, `valori-domain`) vs.
`get_or_init_installation_id` (implementation, `desktop/src-tauri`) already
split.

---

## 14. Migration problem (design only, not implemented)

Walking the task's 10 questions against actual evidence:

1. **Where exactly is the old secret?** `localStorage["valori:llm_config"]`,
   `["valori:embedding_config"]`, `["valori:reranker_config"]` — confirmed
   exact keys and shapes in §3/§4.
2. **Can the desktop app read it safely?** Yes, structurally — a Tauri
   webview's `localStorage` is readable from the JS side that already reads
   it today (`localStorage.getItem`). No new access is needed; the
   migration would be JS-initiated, calling a new Tauri command to hand the
   plaintext value to `CredentialService::create` exactly once.
3. **Can we migrate it automatically?** Yes, mechanically — same one-time,
   idempotent, non-destructive pattern already used twice in this codebase
   (`crate::migration`'s S2a engine for `preferences.json`/`events.jsonl`,
   and `theme.tsx`'s S2c localStorage→`studio.redb` backfill). The precedent
   is: read the legacy value, write it to the new location, **do not delete
   the legacy value yet** (see question 4 below for why).
4. **Can we verify the new credential before deleting the old one?** This is
   the most important open design question. Recommend: after writing to the
   OS keychain, immediately read it back (`CredentialService::get`) and
   compare byte-for-byte to what was written, before ever clearing
   `localStorage`. Only clear `localStorage` on a verified round-trip match
   — mirrors the "verify after commit" step already codified in
   `init_studio_storage_with_paths`'s migration contract (step 4, "Verify",
   `studio-storage.md` §6.5).
5. **What happens if keychain access fails?** (e.g. user denies a macOS
   Keychain access prompt, or Linux has no Secret Service daemon running)
   — the migration must fail closed: leave `localStorage` untouched, leave
   the provider config working exactly as it does today (degrade to the
   pre-S3 behavior), and surface a non-blocking notice (mirroring the
   existing `studio-recovery` event pattern from the DR phase) rather than
   breaking the user's ability to use their configured provider.
6. **What happens if migration is interrupted?** (app killed mid-migration)
   — must be safely resumable from filesystem/keychain state alone, same
   discipline as the DR phase's recovery design ("crash-safe/idempotent,
   derived purely from state, not a flag that can desync from reality").
   Concretely: if `localStorage` still has the value AND the keychain entry
   doesn't verify, retry from scratch; if the keychain entry verifies but
   `localStorage` wasn't cleared yet, that's a safe, resumable
   "verified-but-not-yet-cleaned-up" state, not a failure.
7. **Can migration be retried?** Yes, by design if idempotent per #6 —
   should be a no-op if the keychain entry already exists and verifies.
8. **What happens if multiple provider configurations exist?** (LLM +
   embedding + reranker, each potentially different providers/keys) — each
   needs its own `CredentialRef` and its own keychain entry; migrate all
   three independently, each with its own verify-before-delete step, so a
   failure on one (e.g. reranker) doesn't block or partially corrupt the
   other two.
9. **What happens if the user has already deleted the old localStorage
   value?** (cleared browser data, or a fresh profile) — this is simply
   "no legacy value to migrate," identical in shape to S2a's `source_found:
   false` / "fresh install" case. Not an error.
10. **Can a migration accidentally expose the key in logs or telemetry?**
    Given §6/§7's findings (Rust never receives the key today, and no
    logging/telemetry code path touches provider config), the migration
    Tauri command itself must be written to obey the same discipline —
    **never** `tracing::debug!("migrating key: {}", secret)` or similar.
    This must be an explicit implementation-phase code-review checklist
    item, not just an assumption, precisely because it would be a new
    Rust-side touchpoint that doesn't exist yet.

---

## 15. Backward compatibility

- **Old Studio → New Studio**: a user's existing `localStorage` values keep
  working exactly as they do today until/unless the migration runs and
  verifies successfully (§14 #4-6). No breaking change on upgrade.
- **New Studio → Old Studio (downgrade)**: if a user downgrades after
  migration has cleared `localStorage`, the old Studio build would find
  empty provider config (since it doesn't know how to read the OS keychain)
  — the user would need to re-enter their key. This is the same
  fundamentally-asymmetric downgrade behavior every prior migration phase
  in this codebase already has (e.g. S2b's `studio.redb`-only preferences
  wouldn't be visible to a pre-S2b build reading only `preferences.json`).
  Not a new problem class; consistent with existing precedent. **Whether
  this needs a stronger guarantee (e.g. writing to both locations during a
  transition window) is an open question for the implementation phase.**
- **`project.json`**: `EmbeddingConfig.api_key_ref` already exists in the
  current schema (`#[serde(default, skip_serializing_if = "Option::is_none")]`)
  — populating it for the first time is purely additive, no schema
  migration needed, no version bump needed. Old `valori-daemon` builds
  reading a manifest with a populated `api_key_ref` would simply ignore the
  field (unread field, not unknown-field-rejected, since these are
  `serde_json`-lenient structs throughout this codebase's convention).
- **`studio.redb`**: no schema change is required for the recommended
  design (§11) unless per-provider `credential_ref`s are also cached in
  `StudioPreferences` — if that's chosen, it's a purely additive
  `#[serde(default)]` field, following the exact precedent every prior
  `StudioPreferences` field addition has used, no version bump.

---

## 16. Recovery/backup interaction

The task's framing is exactly right and matches this repository's existing
DR-phase discipline (`studio-storage.md` §10):

```text
studio.redb backup/restore  → credential_ref survives (it's just a string)
OS keychain                 → actual secret lives OUTSIDE studio.redb entirely,
                               so a redb backup/restore never touches it
```

**The consequence the task asks about, confirmed as real**:
- **DB restored on the same machine**: the `credential_ref` resolves fine —
  the OS keychain entry it points to is still there (keychains are
  machine-scoped, not tied to any particular `studio.redb` file).
- **DB copied to another machine**: the `credential_ref` string survives
  the copy (it's just data inside `studio.redb`), but **the keychain entry
  it points to does not exist on the new machine** — resolution would fail.
  This is unavoidable with any OS-keychain-backed design; it is not a flaw
  to fix, it is the correct, expected security property (the whole point of
  using the OS keychain is that the secret does NOT travel with the data
  file).

**Does this require machine-scoped, project-scoped, or
installation-scoped credentials?** Evidence-based answer: **machine-scoped**
is correct and is what `keyring`/OS keychains give by default — a keychain
entry lives on the machine, full stop, independent of which Studio
"installation" or "project" references it. `installation_id` (the prior
phase) is a *separate* concept (Studio's own anonymous identity, stored in
`studio.redb`) and should **not** be conflated with credential scoping —
don't key the keychain entry by `installation_id`, key it by
`(provider, purpose)` or `CredentialRef`, so that if `installation_id` ever
needs to change (e.g. after the "complete loss" fresh-identity case from
the prior phase) existing credentials aren't orphaned. This is a concrete,
evidence-grounded recommendation, not a speculative one — it follows
directly from keeping `InstallationId` and `CredentialRef` as genuinely
separate `valori-domain` concepts with no coupling between them.

**Practical UX implication for the implementation phase (flagged, not
designed)**: moving a project or restoring a backup onto a new machine will
require the user to re-enter provider credentials once, with a clear,
specific error message on first API-call failure ("credential not found on
this machine — re-enter your OpenAI key") rather than a generic auth
failure. This is a UX design item for S3's implementation, not this audit.

---

## 17. Security threat model

| Threat | Localstorage today | With OS keychain (S3 target) |
|---|---|---|
| **`localStorage` file exposure** (anyone with filesystem read access to the user's OS account reads the webview's local storage DB file directly) | **Real, unmitigated** — plaintext JSON, no OS-level access control beyond normal file permissions | Mitigated — secret lives in OS keychain instead, gated by OS-level auth (login, optionally biometric) |
| **Project file exposure** (`project.json` shared/committed/backed up) | Not currently at risk (§4 — `apiKey` is stripped before reaching the daemon) | Stays not-at-risk — `api_key_ref` contains no secret by design |
| **`studio.redb` inspection** (someone opens the redb file directly, e.g. with `dump_studio_db.rs`) | Not currently at risk (§6 — no field exists) | Stays not-at-risk if the design is followed (`credential_ref` only) |
| **Logs** | Not currently at risk (§7 — no logging path touches config) | Must remain not-at-risk — new code (migration, `CredentialService`) must not introduce a logging leak; this is a discipline requirement, not automatic |
| **Telemetry** | Not currently at risk (§6 — no call site populates `payload`/`properties` with config) | Same — structural risk remains (freeform JSON field), needs an explicit test (§20) to keep it that way |
| **Crash reports** | Not currently at risk (§7 — no field, Rust never sees the secret) | Same, contingent on `CredentialService`'s own error handling never embedding the raw secret in an error message passed to the crash marker |
| **Process memory** | The key sits in both the browser/webview process's JS heap and the Next.js server process's memory for the duration of each request — inherent to *any* design that ever uses the key over HTTP, including the OS-keychain design (the key must be read into memory to build the `Authorization` header regardless of where it's stored at rest). **OS keychain does not protect against this** — a debugger or memory-scraping tool with sufficient privilege on the same OS account can still observe the key in-flight, exactly as it can today. | **Not improved by this phase** — explicitly noting this so the eventualS3 rollout doesn't oversell what changed |
| **Malicious local application** (another app on the same machine reading Studio's data) | Can read `localStorage` (§ above) trivially — just a file | With OS keychain: a malicious app **cannot** read another app's keychain entries without its own OS-level authorization/prompt (macOS Keychain ACLs are per-requesting-app by default) — this is the single biggest concrete improvement the OS keychain provides |
| **Keychain access denial** (user says no to a macOS Keychain prompt, or a sandboxed/managed environment blocks it) | N/A — no such prompt exists today | New failure mode introduced by this phase — must degrade gracefully (§14 #5), not silently break provider functionality |
| **Backup exposure** (`studio.redb` backups, per the DR phase) | N/A — no secret in `studio.redb` today | Backups contain only `credential_ref` (§16) — the actual secret is never backed up by Valori's own backup mechanism, which is correct, but means **the user's OS-level keychain backup (e.g. iCloud Keychain sync, if enabled) becomes the only backup of the actual secret** — worth stating explicitly so nobody assumes Studio's DR system covers credentials, because it deliberately won't |
| **Cloud synchronization** | Not applicable — no credential-sync feature exists in this codebase today (checked: no code path uploads `localStorage` provider config to any Cloud/Supabase table) | Same — out of scope unless a future phase explicitly adds Cloud-synced credentials, which would be a materially different (and harder) security design than anything covered here |

**Explicit statement per the task's instruction**: the OS keychain does
**not** make this system "magically secure." It closes the specific,
concrete gap of "any local process/user reading a plaintext file," and nothing
more. It does not protect against a compromised Studio process itself (the
process that legitimately needs the key still holds it in memory to make
the HTTP call), a compromised OS/root-level attacker, or the malicious/
misbehaving "custom" endpoint risk noted in §7.

---

## 18. Future Cloud/hosted-model compatibility

The task raises whether the abstraction should support `CredentialKind`/
`CredentialScope` now. Evidence-based recommendation: **not yet — would be
premature**, consistent with §12's scope conclusion and this project's own
"no speculative abstractions" guideline.

Reasoning: every credential currently in this codebase (§3) reduces to
exactly one shape — `provider + API key, sent as a Bearer token`. There is
no second shape (OAuth token, service account, local-model credential) in
the repository today to design against. Building `CredentialKind`/
`CredentialScope` now would mean guessing at requirements for features that
don't exist yet (hosted inference, marketplace models) — exactly the
"features beyond what was asked" and "flexibility that wasn't requested"
anti-patterns this project's own coding guidelines warn against.

**What *should* be kept in mind without building it**: `CredentialRef`
itself (§12) should be defined as an opaque reference type with no
assumption baked in about what it resolves to being "an API key
specifically" — the *type* can stay generic (an opaque UUID reference) even
though the *service* (`CredentialService`, §13) only knows how to store/
retrieve a single string secret for now. This costs nothing today and
avoids a breaking rename later if a second credential shape does appear.
Do not add `CredentialKind`/`CredentialScope` enums until a second real
shape exists in the codebase to design against.

---

## 19. Implementation phases (proposed, not started)

1. **S3.1 — `CredentialRef` type**: add to `valori-domain` via the existing
   `uuid_id!` macro pattern. Zero behavior change; purely additive type.
2. **S3.2 — `CredentialService`**: add `keyring` dependency to
   `desktop/src-tauri` only; implement create/get/exists/delete; new
   `#[tauri::command]` wrappers; unit + manual-verification tests (keychain
   access can't be fully mocked in CI — needs a real-OS smoke test similar
   to the disposable-`$VALORI_HOME` pattern already used twice in this
   codebase).
3. **S3.3 — Wire into the three UI config hooks**: `useLLMConfig.ts`,
   `useEmbeddingConfig.ts`, reranker config in `SettingsModal.tsx` switch
   from storing `apiKey` directly to storing `credential_ref`, calling the
   new Tauri command to read the secret only at the moment of an actual
   provider HTTP call (embed.ts/llm.ts/reranker.ts's existing `cfg.apiKey`
   parameter becomes populated via a fresh keychain read per call, not
   persisted state) — desktop path only; web/Cloud path keeps today's
   `localStorage` behavior (§8).
4. **S3.4 — Migration**: implement the verify-before-delete flow from §14,
   one-time and idempotent, following the S2a/theme-migration precedent.
5. **S3.5 — Guard rails**: add the tests from §20 (secret never enters
   `studio.redb`/telemetry/logs/crash reports) as permanent regression
   coverage, not just implementation-phase manual checks.

**Explicitly deferred beyond S3** per this audit's evidence: `CredentialKind`/
`CredentialScope` (§18), Cloud-synced credentials, Voyage AI provider
wiring in the UI (§3 — currently Rust-only and not reachable), any change to
Valori Cloud's own (already-correct) API key system (§9/§11).

---

## 20. Tests to recommend (design only, not implemented)

Per the task's list, mapped to what would actually exercise real code:

- `credential stored successfully` / `retrieved successfully` / `deleted` —
  `CredentialService` round-trip, real OS keychain (macOS CI runner at
  minimum; Linux/Windows secondary).
- `missing credential` — `get()` on a `CredentialRef` that was never
  created returns `Ok(None)`, not an error.
- `invalid credential reference` — malformed/unparseable ref string is
  rejected before any OS call is attempted.
- `keychain unavailable` / `keychain permission denied` — both must be
  distinguishable error variants so the UI can show the right message
  (§14 #5); needs platform-specific simulation (e.g. a Linux CI runner with
  no Secret Service daemon running is a natural way to test "unavailable"
  for real, not mocked).
- `migration succeeds` / `interrupted` / `retry` / `idempotency` — mirrors
  the existing `startup_integration.rs` test file's structure
  (`second_startup_is_idempotent_and_performs_no_duplicate_import` is the
  direct precedent to copy the shape of).
- **`secret never serialized into studio.redb`** — an architecture test in
  the same style as this repo's own
  `installation_id_architecture.rs` (shipped in the prior phase): assert no
  `StudioPreferences`/`StudioProjectRecord`/`StudioSessionRecord` field name
  or `set_field` match arm matches a secret-shaped key.
- **`secret never appears in telemetry`** — a property test or fixed-value
  test asserting `StudioTelemetryEvent::new(...)`'s `payload` argument, across
  every real call site, never contains a key matching `/apiKey|api_key|
  secret|token|password/i` — closes the structural risk flagged in §6.
- **`secret never appears in logs`** — harder to test mechanically in Rust;
  recommend a `grep`-based architecture test (same technique as
  `installation_id_architecture.rs`) asserting no `tracing::*!`/`println!`
  call site in `desktop/src-tauri/src/*.rs` interpolates a variable named
  `secret`/`api_key`/`credential` — a naming-convention-dependent but
  cheap, real guard rail.
- **`secret never appears in crash metadata`** — assert `CrashInfo`'s field
  list (already closed/enumerated) never gains a secret-shaped field; a
  simple "these are the only 4 allowed fields" test.
- **`project configuration contains only credential_ref`** — extend the
  existing `EmbeddingConfig` tests (if any exist — **UNKNOWN, not checked
  in this pass**) to assert serialization never includes anything but
  `provider`/`model`/`endpoint`/`api_key_ref`.

---

## 21. Open questions

1. Should `localStorage` be cleared immediately after successful migration,
   or kept as a fallback for some transition period (in case the OS
   keychain later becomes unavailable, e.g. user moves to a locked-down
   managed machine)? (§14 #4, §15)
2. Should the web/Cloud build ever get an equivalent secret-at-rest
   improvement (e.g. server-side proxy so the browser never holds the raw
   key at all), or is `localStorage` accepted as the permanent web-tier
   behavior? Not addressed by this audit — task scope was Desktop-focused.
3. Exact `CredentialRef` serialization prefix (`cred_` vs. bare UUID) —
   cosmetic, but should be decided once, consistently, before
   implementation (§12).
4. Should migrated-but-unverified `localStorage` values ever be force-
   cleared after N failed migration attempts, to avoid an indefinitely
   "half-migrated" state? Not designed here.
5. Does the Linux Secret Service unavailability case (§10/§17) need a
   documented minimum-desktop-environment requirement (e.g. "GNOME Keyring
   or KWallet must be running"), or should Valori ship a pure-file fallback
   for headless Linux? This has real UX cost either way and needs a product
   decision, not just an engineering one.

---

## 22. Explicit UNKNOWNs

Collected from throughout this document, for visibility:

- Whether Voyage AI (`crates/valori-models/src/provider/voyage.rs`) is
  reachable from any currently-wired, live code path, or is dead/unused
  Rust code today. — **UNKNOWN — requires implementation/runtime
  verification.**
- Whether a legacy `~/.valori/projects.json` entry with a pre-migration
  `apiKey` value would actually round-trip unchanged through `touchProject`
  in a real, running app (traced statically, not executed against a real
  legacy fixture in this audit). — **UNKNOWN — requires runtime
  verification.**
- Whether `keyring` actually builds and links cleanly against this
  repository's exact Tauri 2 + existing dependency set (not attempted; this
  audit added no dependencies per its constraints). — **UNKNOWN — requires
  implementation-phase verification.**
- Whether Windows and Linux are actually part of this project's supported/
  tested build matrix today (only macOS-specific dependencies were found
  and confirmed; Windows/Linux support is implied by general Tauri
  conventions but not independently confirmed in this pass). — **UNKNOWN.**
- Whether any `EmbeddingConfig` (Rust) serialization/round-trip tests
  already exist in `valori-daemon` to extend per §20's last test
  recommendation. — **UNKNOWN — not checked in this pass; a follow-up grep
  of `crates/valori-daemon/tests/` before implementation would resolve
  this quickly.**

---

## Final answers

### A. Where is every provider secret currently stored?
`localStorage`, plaintext, three keys: `valori:llm_config`,
`valori:embedding_config`, `valori:reranker_config`. Nowhere else — not in
`studio.redb`, not in `project.json`, not in environment variables (aside
from the unrelated, operator-set `VALORI_EMBED_API_KEY`).

### B. Which code paths can read those secrets?
Only JS/TS code: the three React config hooks/component state
(`useLLMConfig.ts`, `useEmbeddingConfig.ts`, `SettingsModal.tsx`'s reranker
state), and the Next.js server-side modules that receive the config via
POST body and build the outbound `Authorization` header
(`ui/src/lib/server/{embed,llm,reranker}.ts`). No Rust code path can read
them — confirmed by an exhaustive search of every `#[tauri::command]`'s
argument list and every write path into `studio.redb`.

### C. Can any secret currently enter `studio.redb`?
No. `StudioPreferences` has no such field, and the generic key-value
preference bridge (`set_field`) has an exhaustive allowlist that silently
drops any unrecognized key, structurally preventing accidental entry.

### D. Can any secret currently enter telemetry?
Not today — every live call site of the telemetry `send()`/`report*()`
functions passes only timing data. The `payload`/`properties` fields are
freeform JSON, so this is a structural risk to guard against with a test
(§20), not an active leak.

### E. Can any secret currently enter logs or crash reports?
No live path today, on either side. Rust logging/crash-marker code never
receives the secret in the first place (it never reaches Rust — see B).
JS-side logging (`console.*`) has zero occurrences near secret-shaped
terms outside the unrelated Cloud settings audit-log calls (which log only
error messages, never key values).

### F. Which credentials belong to Desktop vs Cloud?
Local provider credentials (OpenAI/Cohere/Groq/Together/custom) are
**Shared** between Desktop and Web/Cloud builds — same code path, same
`localStorage` mechanism, different browser/webview instances so the
values themselves don't sync. Valori Cloud authentication (Supabase
session, Cloud API keys, personal access tokens) is **Cloud-only** — a
completely separate, already-correctly-designed (hash-stored, reveal-once,
cookie session) system that never touches the Desktop/Tauri/Rust layer.

### G. What OS credential-store implementation should Valori use?
The `keyring` crate (MIT/Apache-2.0, actively maintained, covers macOS
Keychain / Windows Credential Manager / Linux Secret Service with one
synchronous API). No suitable dependency exists in the repo today; this
would be a new, single, `desktop/src-tauri`-only dependency.

### H. What should `CredentialRef` look like?
A UUID v4, defined in `valori-domain` via the existing `uuid_id!` macro
(same pattern as `InstallationId`), serialized the same way every other
`valori-domain` ID is. No `CredentialKind`/`CredentialScope` yet —
premature given only one credential shape exists in the codebase today.

### I. How should existing plaintext credentials migrate without loss?
One-time, idempotent, resumable migration: read the `localStorage` value →
write it to the OS keychain via `CredentialService::create` → read it back
and verify a byte-for-byte match → only then clear the `localStorage`
value. Fail closed (leave `localStorage` untouched) on any keychain error,
so the user's provider functionality never breaks mid-migration. Full
10-point design in §14.

### J. How does the design interact with Studio DB backup/recovery?
`credential_ref` (a plain string) survives `studio.redb` backup/restore
exactly like any other field — no special handling needed. The actual
secret deliberately does **not** live in `studio.redb` and is therefore
**not** covered by Studio's own DR system at all; it is implicitly
machine-scoped via the OS keychain, and a `studio.redb` copied to a new
machine will have `credential_ref`s that no longer resolve there — expected
and correct, requiring only a clear re-entry UX, not a data-recovery fix.

### K. What should we implement in S3, and what should remain deferred?
**Implement**: `CredentialRef` type (`valori-domain`), `CredentialService`
(`desktop/src-tauri`, using `keyring`), wiring into the three existing
config surfaces (desktop/Tauri path only), the verify-before-delete
migration, and the guard-rail tests from §20 (especially the
"secret never enters telemetry/studio.redb/logs" architecture tests, which
are cheap and directly prevent regression of what this audit confirmed is
currently safe).
**Defer**: `CredentialKind`/`CredentialScope`, any Cloud-side credential-
storage change (already correct), Voyage AI UI wiring, a browser/Cloud-tier
secret-at-rest improvement, and any headless-Linux-specific fallback design
beyond documenting the limitation.

---

*End of audit. No source files, dependencies, `studio.redb` contents,
`localStorage` contents, or project manifests were modified. Awaiting
approval before any implementation work begins.*
