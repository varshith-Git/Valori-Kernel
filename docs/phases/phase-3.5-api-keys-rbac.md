# Phase 3.5 — Per-tenant API Keys + RBAC

## Goal

Replace the single shared `VALORI_AUTH_TOKEN` with a proper multi-key credential
system: scoped tokens (read-only, read-write, admin), JSON-file persistence, full
CRUD management API, and a unified auth middleware that works identically on
standalone and cluster nodes.

## Delivered

### `crates/valori-node/src/api_keys.rs` (new module)

**`ApiScope`** — three-tier enum (`read_only < read_write < admin`) with
`satisfies(required)` for scope comparison.  Serializes as `"read_only"`,
`"read_write"`, `"admin"` (snake_case).

**`ApiKeyRecord`** — stored representation: id (hex prefix `key_<16 hex>`),
scope, optional collection lock, optional description, unix `created_at`,
BLAKE3 hash of the raw token, and the first 8 characters of the token as a
`prefix` field for operator identification (never the full token).

**`ApiKeyCreated`** — response returned exactly once at creation, the only time
the plain-text token is exposed.

**`ApiKeyMasked`** — safe list/get representation: omits `token_hash` and raw
token bytes.

**`KeyStore`** — thread-safe, two-index store:
- `by_hash: RwLock<HashMap<[u8; 32], ApiKeyRecord>>` — O(1) auth lookup
- `id_to_hash: RwLock<HashMap<String, [u8; 32]>>` — O(1) revocation

Methods: `create()`, `revoke()`, `list()`, `lookup()`, `is_empty()`.
Persistence: on every write, the full set is serialized to a JSON file at
`VALORI_KEYS_PATH`; on startup, the file is loaded back.

**Token format**: `vk_` + 64 hex chars (128 bits of OS random via `/dev/urandom`).
Stored as BLAKE3 hash — the raw token is never written to disk.

**`AuthState`** — shared between the middleware and key-management handlers:
holds `Arc<KeyStore>` + `legacy_token: Option<String>`.

**`required_scope(method, path)`** — route classification without per-handler
changes:
- `/v1/keys/*`, `/v1/snapshot/*`, `/v1/storage/*` → Admin
- `POST /search`, `GET *`, read-only helpers → ReadOnly
- All other POST / DELETE → ReadWrite

### `crates/valori-node/src/server.rs`

- **`auth_guard_v2`** — new async middleware (axum `from_fn`) that:
  1. Skips auth entirely when `AuthState.has_any_auth() == false`.
  2. Classifies the request scope via `required_scope()`.
  3. Checks the presented Bearer token against `KeyStore.lookup()`.
  4. Falls back to the legacy `VALORI_AUTH_TOKEN` (treated as admin).
  5. Returns 401 on missing/wrong token, 403 on insufficient scope.
- **`build_router_with_keys(state, auth_token, cors_origin, key_store)`** — new
  public entry point used by `main.rs`. Wires the auth Extension before the
  middleware layer (Extension must be the outermost layer so it is injected
  before the guard runs).
- **`build_router()`** — unchanged signature; internally calls
  `build_router_with_keys` with an in-memory `KeyStore::new(None)`. All existing
  tests continue to call this.
- **New handlers**: `create_key_handler`, `list_keys_handler`, `revoke_key_handler`
  on `/v1/keys` and `/v1/keys/:id`.

### `crates/valori-node/src/cluster_server.rs`

- **`cluster_auth_guard`** — identical logic to `auth_guard_v2`.
- **`build_cluster_router_with_keys()`** — new entry point with auth + key store.
- **`build_cluster_router()`** — now delegates to `build_cluster_router_with_keys`
  with an empty key store (backward compat).
- **New cluster handlers**: `cluster_create_key`, `cluster_list_keys`,
  `cluster_revoke_key` on the same `/v1/keys` routes.

### `crates/valori-node/src/config.rs`

- Added `keys_path: Option<PathBuf>` field.
- Reads `VALORI_KEYS_PATH` env var at startup.

### `crates/valori-node/src/main.rs`

- Creates `Arc<KeyStore>` from `cfg.keys_path` before building the router.
- Calls `build_router_with_keys` instead of `build_router`.

### `crates/valori-node/src/lib.rs`

- Added `pub mod api_keys`.

### `crates/valori-node/tests/api_keys.rs` (new)

Eight integration tests:
1. `no_auth_all_requests_pass` — no token configured → all requests succeed.
2. `legacy_token_accept_and_reject` — `VALORI_AUTH_TOKEN` still works; wrong
   token → 401.
3. `create_key_and_use_it` — create a `read_write` key via admin token, use it
   for insert + search, find it in the list.
4. `list_keys_requires_admin` — `read_only` key → 403 on `GET /v1/keys`.
5. `read_only_key_cannot_write` — `read_only` key → 403 on POST /records, 200
   on POST /search.
6. `revoke_key_stops_access` — after DELETE /v1/keys/:id, the key is rejected.
7. `revoke_nonexistent_key_returns_404`.
8. `health_always_public` — `/health` responds 200 even when auth is configured.

## Findings

1. **Collection-scope enforcement is stored but not enforced at the data layer**
   — `ApiKeyRecord.collection` is persisted and returned in the list response,
   but the middleware does not check it against the `"collection"` field in
   POST /records, /search etc. Enforcing this requires parsing the request body
   in middleware, which axum discourages (consuming the body in middleware makes
   it unavailable to the handler). The recommended approach is to extract the
   collection as a URL path segment (e.g. `/v1/collections/:name/records`) — a
   breaking API change deferred to Phase 4+.

2. **Key store is not Raft-replicated** — on a cluster, keys created via the
   leader are persisted to that node's `VALORI_KEYS_PATH` but are NOT propagated
   to followers. A follower that handles a write request will forward it to the
   leader (307), which may or may not have the key. For full cluster-wide ACL
   enforcement, a `KernelEvent::CreateApiKey` / `RevokeApiKey` pair should be
   added in Phase 3.9 (Operations automation) or a dedicated Phase 4 security
   hardening phase.

3. **BLAKE3 vs Argon2id** — the roadmap specified Argon2id for key hashing.
   Since keys are 128-bit random tokens (not passwords), Argon2id's
   password-stretching properties are unnecessary. BLAKE3 is sufficient and
   keeps the dependency tree clean (blake3 is already a dep; argon2 is not).
   If user-provided passwords are ever accepted as key material, migrate to
   Argon2id.

4. **Non-unix token generation** — the `#[cfg(unix)]` branch reads from
   `/dev/urandom`. The `#[cfg(not(unix))]` fallback (time+PID hash) is NOT
   cryptographically secure. Production deployments must run on Linux/macOS.

## Validation

```
cargo test -p valori-node --test api_keys
```

```
running 8 tests
test revoke_nonexistent_key_returns_404 ... ok
test health_always_public ... ok
test no_auth_all_requests_pass ... ok
test legacy_token_accept_and_reject ... ok
test list_keys_requires_admin ... ok
test revoke_key_stops_access ... ok
test create_key_and_use_it ... ok
test read_only_key_cannot_write ... ok

test result: ok. 8 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.02s
```

Full suite: all tests pass, zero regressions.

## Follow-ups

- **Collection-scope enforcement** — add `collection` as a URL path segment to
  enable middleware-level collection filtering without body parsing.
- **Raft replication of keys** — `KernelEvent::CreateApiKey / RevokeApiKey`
  so all cluster nodes share the same ACL table.
- **Key expiry** — add `expires_at: Option<u64>` to `ApiKeyRecord`; enforce in
  `KeyStore::lookup()`.
- **Python SDK** — `SyncRemoteClient.create_key()`, `list_keys()`,
  `revoke_key()` methods.
