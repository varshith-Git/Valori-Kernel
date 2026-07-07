# Phase S18 — V4 per-entry CRC32 for inline corruption detection

## Goal

Eliminate the silent-corruption window where a bit-flipped event log entry decodes
successfully as valid bincode but applies wrong data to the kernel. The BLAKE3 chain
catches this only when the *next* entry is decoded (its `prev_hash` won't match) or
when `valori-verify` replays the full chain. Add a cheap per-entry CRC32 suffix that
catches corruption of any entry — including the most recent one — at read time.

## Delivered

### `crates/valori-wire/Cargo.toml`

Added `crc32fast = "1.3"`.

### `crates/valori-wire/src/lib.rs`

- `VERSION_V4 = 4` — new segment version.
- `HEADER_SIZE_V4 = HEADER_SIZE_V3 = 48` — same header layout, version byte differs.
- `CRC32_SUFFIX_LEN = 4` — byte length of the per-entry suffix.
- `EntryV4 = EntryV3` — identical fields; the CRC is outside the bincode payload.
- `encode_header_v4()` — V3-layout header with `VERSION_V4` in bytes 0-3.
- `parse_header()` — V4 arm added; accepted as a valid segment version.
- `encode_entry(VERSION_V4, …)` — encodes bincode payload then appends `CRC32(payload)` as 4-byte LE.
- `decode_entry(VERSION_V4, …)` — decodes bincode, reads 4-byte CRC suffix, rejects if mismatch → `WireError::Decode("V4 entry CRC32 mismatch: …")`.
- `chain_advance(VERSION_V4, …)` — delegates to `chain_advance_v3` (CRC is transport-only, not part of the chain hash; V4 chains are identical to V3).

### `crates/valori-storage/src/events/event_log.rs`

- New files open as V4 (was V3).
- Rotated segments open as V4.
- `request_id` passthrough condition changed from `== VERSION_V3` to `>= VERSION_V3`.
- Two test assertions updated: `"new files are v3"` → `"new files are v4"`.

### `crates/valori-node/src/events/event_log.rs`

Same changes as `valori-storage` (duplicate of the same file; both must stay in sync).

### `crates/valori-wire/tests/hardening.rs`

Six new tests:

| Test | What it verifies |
|---|---|
| `v4_roundtrip_clean` | Clean V4 entry encodes and decodes; consumed byte count includes CRC suffix |
| `v4_crc_suffix_present` | Encoded V4 bytes are longer than just the CRC suffix |
| `v4_bit_flip_in_payload_is_caught` | A single flipped bit in the bincode payload → `WireError::Decode` |
| `v4_crc_suffix_tamper_is_caught` | A tampered CRC suffix itself → `WireError::Decode` |
| `v4_truncated_crc_suffix_is_caught` | One byte removed from CRC suffix → `WireError::Decode` |
| `v4_chain_advance_matches_v3_formula` | Chain hash of identical V3 and V4 entries are byte-equal |

## Design decisions

- **CRC32 not BLAKE3 for the suffix.** CRC32 is a ~0.3 ns/byte error-detection code;
  BLAKE3 is a cryptographic hash. The chain already provides tamper detection via BLAKE3.
  The CRC is a separate, cheap layer for catching storage-layer bit rot (RAM ECC errors,
  flash block errors), not adversarial tampering. Using BLAKE3 for both would double the
  hash cost per write for no security gain.
- **CRC outside the bincode payload.** The bincode blob is identical between V3 and V4;
  `EntryV4 = EntryV3`. This means existing V3 decode logic is untouched and the chain
  formula is unchanged.
- **V2 and V3 segments remain fully readable.** `decode_entry` has no CRC check for
  v2/v3 — old logs decode exactly as before.
- **The duplicate `event_log.rs`** in `valori-node/src/events/` and
  `valori-storage/src/events/` must be kept in sync manually until the node's re-export
  is completed.

## Validation

- `cargo test -p valori-wire`: **15 passed** (was 9; +6 V4 CRC tests), **0 failed**
- `cargo test -p valori-node --test cluster_namespaces`: **16 passed, 0 failed**
- `cargo test -p valori-node --test api_keys`: **8 passed, 0 failed**
- `cargo test -p valori-node --test engine_snapshot_roundtrip`: **5 passed, 0 failed**
- `cargo test -p valori-node --test persistence_tests`: **1 passed, 0 failed**
- `cargo build -p valori-kernel --target wasm32-unknown-unknown`: passes (wire/kernel boundary unchanged)

## Follow-ups

| Item | Notes |
|---|---|
| Generate V4 fixture file in `tests/fixtures/` | `generate_fixtures` test (currently ignored) should be run once to add `v4.bin`; then add a `v4_fixture_decodes_forever` test mirroring `v3_fixture_decodes_forever` |
| Unify the duplicate `event_log.rs` | `valori-node` re-exports from `valori-storage`; the node copy is dead weight |
| `valori-cli verify` should report V4 and check CRC inline | Currently only checks BLAKE3 chain; CRC violations in V4 will surface as chain breaks but a dedicated CRC column in the output would help operators diagnose storage vs. tampering |
