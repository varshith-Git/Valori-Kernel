# Phase 1.5 — Crypto-shredding design (GDPR)

**Status:** done · on `multinode`
**Roadmap:** [MULTINODE_ROADMAP.md](../MULTINODE_ROADMAP.md) § 1.5

## Goal

Reserve the schema positions for GDPR "right to erasure" **now**, while the
wire format and event schema are still in active motion — so that the actual
encryption implementation lands as additive code, not a migration.

GDPR Article 17 demands that personal data can be erased on request.
For an append-only event log the only viable design is **crypto-shredding**:
encrypt the sensitive payload at write time, then destroy the key.  The log
entry stays forever (preserving the audit chain and the chain hash); the
content becomes permanently unreadable.  A verifier sees "record present,
content unrecoverable" — the chain head still commits to it.

## Delivered

### `crates/valori-kernel/src/crypto/mod.rs` (new)

Defines the cryptographic abstraction layer:

| Symbol | What it is |
|---|---|
| `KeyId = [u8; 16]` | 128-bit key identifier (random UUID-v4 per record) |
| `CryptoError` | `KeyNotFound`, `EncryptionFailed`, `DecryptionFailed`, `BackendError` |
| `trait KeyVault` | `encrypt / decrypt / shred / key_exists` — one impl per backend |
| `NullVault` | Stub that panics on any call (`key_exists` returns `false`) |

**`KeyVault` invariant** — after `shred(key_id)` returns `Ok`, `decrypt(key_id,
_)` must return `Err(KeyNotFound)` even across process restarts.  The vault
backend (in-process AES-GCM map, AWS KMS, HashiCorp Vault, …) is responsible
for this durability guarantee; the trait just names the contract.

`NullVault` is the default until a real vault is configured.  Because it panics
on any actual cryptographic call, it is impossible to accidentally write an
`InsertRecordEncrypted` event with the stub in place — the panic surfaces the
misconfiguration immediately rather than silently writing "encrypted" records
whose payload is actually plaintext.

### Two reserved `KernelEvent` variants (event.rs)

| Variant | Index | Payload |
|---|---|---|
| `InsertRecordEncrypted` | 7 | `id`, `key_id: [u8;16]`, `ciphertext: Vec<u8>`, `metadata_ciphertext: Option<Vec<u8>>`, `tag` |
| `ShredKey` | 8 | `key_id: [u8;16]` |

Per the evolution policy: new variants are append-only (indices 7 and 8 are
now permanently reserved), existing variants 0–6 are unchanged, and both new
variants round-trip through bincode correctly.

`apply_event()` returns `Err(KernelError::NotImplemented)` for both variants
today, so no encrypted data can enter the state machine until the vault is
wired in.  The match arms in `server.rs`, `timeline.rs`, and `valori-ffi`
are all updated to display the new variants legibly.

### `KernelError::NotImplemented` (error.rs)

New variant — used by the reserved event arms and available for future phase
stubs.

### `crates/valori-kernel/tests/crypto.rs` (new, 11 tests)

| Test | What it proves |
|---|---|
| `key_id_is_16_bytes` | Type alias is the right width |
| `null_vault_key_exists_is_always_false` | Stub correctly reports no keys |
| `null_vault_encrypt/decrypt/shred_panics` | Stub panics loudly (3 tests) |
| `insert_record_encrypted_variant_serializes` | Variant 7 round-trips bincode |
| `shred_key_variant_serializes` | Variant 8 round-trips bincode |
| `insert_record_encrypted_is_refused_by_apply` | `NotImplemented`, no state mutation |
| `shred_key_is_refused_by_apply` | `NotImplemented` |
| `crypto_error_display_names_the_key` | Error message includes hex key prefix |
| `key_vault_is_object_safe` | `&dyn KeyVault` compiles |

## Design decisions recorded

**Why erase = key destruction, not log truncation?**

The audit log's value is precisely that it cannot be truncated.  If an auditor
needs to verify "record #4711 was present until 2027-03-15", log truncation
destroys that evidence.  Crypto-shredding preserves it: the chain head commits
to the encrypted blob, the blob is in the log, and the inability to decrypt is
the erase evidence.  The auditor can confirm "record written, key destroyed,
content unrecoverable" — which is exactly what GDPR Article 17 requires.

**Why `key_id` per record, not per user?**

A per-user key means a single `shred` erases all of that user's records at
once.  That sounds convenient but creates a covert deletion channel: an
operator can shred all records for a class of users with one key op.
Per-record keys make every erase operation explicit, auditable, and
individually logged in the event stream (`ShredKey` is itself a durable event).

**Why `InsertRecordEncrypted` rather than adding `key_id` to `InsertRecord`?**

The evolution policy forbids modifying existing variant fields.  A new variant
is the correct pattern — and it makes the distinction visible in replay: a
viewer that sees `InsertRecord` knows the payload is plaintext; one that sees
`InsertRecordEncrypted` knows a vault decrypt is needed.

**What gets encrypted?**

The `ciphertext` field covers the vector.  `metadata_ciphertext` covers the
metadata blob.  The `tag` and `id` remain in plaintext — they are routing
metadata (like an envelope address), not personal data.  The key_id is also
plaintext; knowing which key encrypted a record reveals nothing without the key.

**AEAD vs. deterministic encryption?**

The vault must use an authenticated cipher (AES-256-GCM or ChaCha20-Poly1305).
This prevents an attacker from substituting a valid ciphertext from another
record under the same key.  Non-determinism of the nonce is fine: the nonce is
embedded in the `ciphertext` bytes, so replay is always possible as long as the
key exists.

## Findings

No bugs — this phase is pure addition. One ripple: every existing exhaustive
`match` on `KernelEvent` in `server.rs`, `timeline.rs`, and `valori-ffi`
needed new arms.  This is the expected cost of a non-wildcard enum and is
itself a safety feature: the compiler will catch any future code path that
forgets to handle the encrypted variants.

## Validation

- Full suite: **185 tests passing, 0 failures**
- 11 new tests in `tests/crypto.rs` covering types, stubs, schema, apply rejection,
  error display, and trait object safety

## Follow-ups

- **Key vault implementation** (future phase): AES-256-GCM in-process implementation
  with a pluggable key-store backend (file, KMS, HSM)
- **Key-store durability**: shredded-key set must survive process restart;
  a simple append-only shred-log file (or a small SQLite) works
- **API surface**: `POST /records/encrypted`, `DELETE /keys/{key_id}` endpoints
- **Recovery validation**: replay must treat a `ShredKey` event as "forget
  this key" — any subsequent `InsertRecordEncrypted` with that key_id must
  fail to decrypt, not crash
- **Verifier extension** (Phase 1.7): the verifier should display
  `[SHREDDED — key {hex} destroyed YYYY-MM-DD]` for unrecoverable records
