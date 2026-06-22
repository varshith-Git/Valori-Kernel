# Phase 3.6 — Crypto-shredding (GDPR Erasure)

## Goal

Implement AES-256-GCM per-record encryption with cryptographic erasure: destroying a Data Encryption Key (DEK) makes all data encrypted under it permanently unrecoverable, satisfying GDPR Article 17 "right to erasure" without truncating the audit log.

## Delivered

### `crates/valori-kernel`

| File | Change |
|---|---|
| `src/storage/record.rs` | Added `is_searchable()` method (excludes `FLAG_ENCRYPTED | FLAG_SHREDDED` from search); updated `is_active()` to also exclude `FLAG_SHREDDED`; updated flag constant docstrings to reflect Phase 3.6 implementation |
| `src/storage/pool.rs` | Added `mark_encrypted()` and `mark_shredded()` methods; added `FLAG_ENCRYPTED` and `FLAG_SHREDDED` to imports |
| `src/state/kernel.rs` | Added `encrypted_record_keys: FxHashMap<[u8;16], Vec<RecordId>>` field to `KernelState`; added `apply_shred_key()` method; implemented `KernelEvent::InsertRecordEncrypted` and `KernelEvent::ShredKey` arms in `apply_event_ns()`; implemented `Command::InsertRecordEncrypted` and `Command::ShredKey` arms in `apply()` |
| `src/state/command.rs` | Added `InsertRecordEncrypted` and `ShredKey` variants (removed the reservation comments) |
| `src/event.rs` | Added `AutoInsertRecordEncrypted` variant for cluster-mode; added to `event_type()`, serialize/deserialize, and `KernelEventHelper` |
| `tests/crypto.rs` | Updated Phase 1.5 "refused" tests to Phase 3.6 "implemented" tests; added `insert_record_encrypted_applies_and_sets_flag` and `shred_key_sets_shredded_flag_on_all_matching_records` |

### `crates/valori-node`

| File | Change |
|---|---|
| `Cargo.toml` | Added `aes-gcm = "0.10"` dependency |
| `src/crypto_vault.rs` | **NEW** — `AesGcmVault` struct: in-memory or shred-log-backed vault; `encrypt()`, `decrypt()`, `shred()`, `key_exists()`; `new_key_id()`, `key_id_to_hex()`, `hex_to_key_id()` helpers |
| `src/lib.rs` | Added `pub mod crypto_vault` |
| `src/config.rs` | Added `shred_log_path: Option<PathBuf>` field, parsed from `VALORI_SHRED_LOG_PATH` env var |
| `src/engine.rs` | Added `vault: Arc<dyn KeyVault>` field to `Engine`; vault wired from config in `Engine::new()`; added `insert_encrypted_ns()` and `shred_key()` methods; updated `build_index()` to use `is_searchable()` instead of `is_active()` |
| `src/server.rs` | Added routes: `POST /v1/records/encrypted`, `DELETE /v1/crypto/shred/:key_id`, `GET /v1/crypto/status/:key_id`; added handler functions |
| `src/cluster_server.rs` | Added `vault: Arc<dyn KeyVault + Send + Sync>` to `DataPlaneState`; added same three routes; added `cluster_insert_encrypted`, `cluster_shred_key`, `cluster_crypto_status` handlers |
| `tests/api_crypto_shred.rs` | **NEW** — 5 integration tests (see Validation) |

### `python/valoricore`

| File | Change |
|---|---|
| `remote.py` | Added `insert_encrypted()`, `shred_key()`, `shred_key_status()` to both `SyncRemoteClient` and `AsyncRemoteClient` |

## Findings

1. **Dim must be set before encrypted insert**: `InsertRecordEncrypted` requires `KernelState.dim` to be set (so the zero vector can be sized correctly). The HTTP handler returns a clear 500 if this isn't set yet. In practice, callers should prime the dimension via a normal insert or ensure `VALORI_DIM` is set at startup.

2. **Cluster vault is per-node**: In cluster mode, the DEK lives only on the node that encrypted the record (the leader at write time). Other replicas store the ciphertext in their kernel but cannot decrypt it. `ShredKey` propagates through Raft to set `FLAG_SHREDDED` on all replicas; the DEK is shredded on the leader only (which is the only node that could decrypt anyway).

3. **`AutoInsertRecordEncrypted` added**: A new `KernelEvent` variant for cluster-mode encrypted inserts, analogous to `AutoInsertRecord` — the state machine assigns the record ID deterministically.

4. **Shred log durability**: Shredded key_ids are appended (hex) to `VALORI_SHRED_LOG_PATH` so they remain unrecoverable across restarts. The in-memory vault (no path configured) only survives the process lifetime.

## Validation

```
cargo test -p valori-kernel -p valori-node
test result: 220 passed; 0 failed
```

New tests in `tests/api_crypto_shred.rs`:
- `test_insert_encrypted_returns_key_id` — POST returns 201 with `id` and 32-char hex `key_id`
- `test_shred_key_makes_status_return_false` — vault status goes from `exists=true` to `exists=false` after shred
- `test_encrypted_record_not_in_search_results` — encrypted (zero) vector does not pollute nearest-neighbour search
- `test_bad_key_id_format_returns_400` — invalid hex key_id rejected with 400
- `test_encrypt_two_records_under_same_key_then_shred` — multiple records under one key, shredded atomically

Updated kernel tests in `tests/crypto.rs`:
- `insert_record_encrypted_applies_and_sets_flag` — `FLAG_ENCRYPTED` set; `FLAG_SHREDDED` not set; zero vector confirmed
- `shred_key_sets_shredded_flag_on_all_matching_records` — both records get `FLAG_SHREDDED`; metadata wiped

## Follow-ups

| Deferred | Phase |
|---|---|
| Replicate vault DEKs across Raft cluster (key-rotation protocol) | 3.6.1 / future |
| `GET /v1/records/:id/decrypt` — retrieve plaintext given key_id | 3.6.1 / future |
| Snapshot encode/decode should persist `encrypted_record_keys` | 3.6.1 |
| Python SDK: `ClusterClient.insert_encrypted()` | 3.6 follow-up |
