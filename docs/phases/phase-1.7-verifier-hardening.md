# Phase 1.7 — Verifier Hardening: Decode Limits, Fuzzing, Multi-Segment, zstd

**Status:** planned  
**Roadmap:** [MULTINODE_ROADMAP.md](../MULTINODE_ROADMAP.md) § 1.7  
**Why now:** `valori-verify` is a security-critical binary whose *entire purpose*
is to parse attacker-controlled bytes. It reads files from untrusted sources —
auditors, customers, forensic scenarios — and the current code makes no
assumption about the file being well-formed beyond length checks. Any
deserialisation-time panic or OOM is a denial-of-service against the audit
capability. Phase 1.8 adds zstd-compressed segments; the verifier must be
ready to read them on the same release.

---

## Goal

1. **Decode limits** — cap every allocation bincode makes before touching data,
   so a crafted 8-byte file cannot OOM the verifier.
2. **Fuzzing** — `cargo-fuzz` target with a short CI smoke run so the decode
   path is continuously exercised against novel byte patterns.
3. **Multi-segment verification** — follow `prev_segment_chain_head` splices
   across archived segment files; a whole segment cannot be silently removed.
4. **zstd read** — transparent decompression of sealed (compressed) segments
   so the verifier works against the storage layout Phase 1.8 introduces.
5. **Acceptance gate** — fuzzer runs N minutes in CI without panics or OOM;
   crafted oversized-allocation files are rejected with a clear error, not an
   OOM kill.

---

## Problem: Current Decode Path Has No Allocation Caps

### What bincode does today

```rust
// valori-wire/src/lib.rs — current call site
let (e, n): (EntryV3, usize) =
    bincode::serde::decode_from_slice(bytes, cfg())
        .map_err(|e| WireError::Decode(e.to_string()))?;
```

`cfg()` returns `bincode::config::standard()`, which uses the default
`Limit::Unlimited`. A crafted entry can encode a `Vec<u8>` with a claimed
length of `usize::MAX`; bincode allocates immediately, causing an OOM before
the error path is ever reached.

### Attack surface

The verifier reads a file path from the command line — any file, from any
source. The threat model (§1.6) explicitly identifies "crafted oversized-
allocation log files" as an in-scope risk. The verifier is also callable from
CI automation and auditor tooling where the process memory limit may not be
artificially restricted.

---

## D1 — bincode Allocation Limit

### Configuration constant

```rust
// valori-wire/src/lib.rs

/// Maximum bytes bincode may allocate while decoding a single log entry.
///
/// An entry carries: prev_hash (32 B) + wall_time_secs (8 B) +
/// request_id Option (17 B) + LogEntry payload.
/// The largest legitimate payload is an InsertRecord with dim=4096 and
/// metadata up to METADATA_CAP bytes (see D2). 4096 * 4 bytes per
/// FxpScalar + metadata cap + framing ≈ 32 KB overhead is generous.
///
/// This cap is applied to every bincode decode in valori-wire and
/// valori-verify. It does NOT apply to the kernel-internal snapshot decoder
/// (which is called by trusted paths only and uses its own limits).
pub const MAX_ENTRY_DECODE_BYTES: u64 = 1 << 20; // 1 MiB per entry
```

### Updated `cfg()` helper

```rust
fn cfg() -> impl bincode::config::Config {
    bincode::config::standard()
        .with_limit::<{ MAX_ENTRY_DECODE_BYTES }>()
}
```

`with_limit` is available in bincode 2.x via the `Limit` trait. The limit
applies to all `Vec` and `String` allocations inside a single decode call;
if any allocation exceeds the limit, `decode_from_slice` returns
`DecodeError::LimitExceeded` without allocating.

### Error surfacing

`WireError` gains a new variant:

```rust
#[error("entry at byte offset {offset} claims an allocation larger than the \
         {MAX_ENTRY_DECODE_BYTES}-byte decode limit — file is likely crafted")]
DecodeLimitExceeded { offset: usize },
```

The verifier maps this to a new `Failure::DecodeLimitExceeded` and emits:

```
❌  TAMPERED (structural — allocation limit exceeded)
    entry #N at byte offset X claims an allocation > 1 MiB.
    This indicates a crafted or corrupted file, not a valid event log.
```

This is classified as `tampered_structural` in the JSON report (same bucket as
corrupt-decode, which is already user-visible and auditable).

---

## D2 — Entry-Level Sanity Bounds

In addition to the overall allocation limit, add per-field guards that reject
logically impossible values before bincode ever reads the payload bytes:

| Field | Guard | Rationale |
|---|---|---|
| `dim` in segment header | `1 ≤ dim ≤ 32_768` | No real embedding is 0-dim or >32K |
| `segment_seq` in header | `≤ u32::MAX - 1` (no-op today, documents the contract) | Wrap-around would break splice ordering |
| `Vec<u8>` metadata in an `InsertRecord` event | `≤ METADATA_CAP = 65_536` | 64 KiB per record is already generous |
| Vector dimension inside an event | Must equal header `dim` | Mismatch = structural corruption |
| Number of entries decoded from a single segment | `≤ MAX_ENTRIES_PER_SEGMENT = 10_000_000` | 10M entries × min 40 B = 400 MB — already enormous; prevents infinite loops on circular data |

These checks go in `valori-wire`'s decode path so both the node (on recovery)
and the verifier benefit simultaneously.

---

## D3 — `cargo-fuzz` Target

### Fuzz crate layout

```
crates/valori-fuzz/           [NEW — Phase 1.7]
  Cargo.toml
  fuzz_targets/
    fuzz_verify.rs            — entry point: fuzz one segment file end-to-end
    fuzz_header.rs            — header-only parsing (faster, higher coverage)
    fuzz_entry.rs             — single-entry bincode decode
```

`valori-fuzz` is a separate crate (not in the workspace default-members) so
it doesn't affect `cargo build` or `cargo test` by default. CI runs it
explicitly with a time budget.

### `fuzz_verify.rs` target

```rust
#![no_main]
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    // Must not panic, abort, or OOM regardless of input.
    // Errors (WireError, Failure) are acceptable — they are the expected result
    // for malformed input.
    let _ = valori_wire::parse_header(data);
    if let Ok(header) = valori_wire::parse_header(data) {
        let body = &data[header.header_len..];
        let mut offset = 0;
        let mut count = 0u64;
        while offset < body.len() && count < 10_000 {
            match valori_wire::decode_entry(header.version, &body[offset..]) {
                Ok((_, n)) => offset += n,
                Err(_)     => break,
            }
            count += 1;
        }
    }
});
```

The invariant: **no panic, no abort, no OOM** for any input. Errors are fine.

### CI integration

```yaml
# .github/workflows/fuzz.yml
- name: Fuzz verifier (smoke run)
  run: |
    cargo install cargo-fuzz
    cd crates/valori-fuzz
    cargo fuzz run fuzz_verify -- -max_total_time=60 -max_len=65536
  # 60-second smoke run. Real overnight runs in a separate workflow
  # on release branches (10 minutes per target).
```

The corpus is seeded from the fixture logs under `crates/valori-wire/tests/fixtures/`
so the fuzzer starts from valid examples and mutates toward edge cases.

### Acceptance criterion

No new crashes or OOM conditions in the seed corpus after the decode limit
and sanity bound patches land. The 60-second CI run serves as a regression
gate; the overnight schedule is a continuous hardening effort.

---

## D4 — Multi-Segment Verification

### Current state

`valori-verify` accepts exactly one file path and verifies it in isolation.
The v3 header carries `prev_segment_chain_head` — the final chain hash of the
*previous* segment — but the verifier never checks whether that value actually
matches any file on disk. An attacker can delete an entire archived segment
without breaking any verification that only looks at a single file.

### New `--log-dir` mode

```
valori-verify --log-dir /path/to/data_dir/audit/
valori-verify --log-dir /path/to/data_dir/audit/ --expected-hash <hex>
```

When `--log-dir` is given:

1. **Enumerate** all files matching `events*.log` (or a configurable glob via
   `--segment-glob`), sorted by `segment_seq` ascending.
2. **Verify each segment** individually (chain + replay).
3. **Verify splices** — after segment N is verified, check that segment N+1's
   `prev_segment_chain_head` equals segment N's final `chain_head`. If not,
   report:
   ```
   ❌  MISSING OR SUBSTITUTED SEGMENT
       segment #N+1 splices to chain head <hex>
       but segment #N ends at chain head <hex>
       Either segment #N was deleted or segment #N+1 was written from a
       different log history.
   ```
4. **Accumulate kernel state** — replay events across segments in order,
   carrying the `KernelState` forward. The final state hash covers the entire
   history.
5. **Report** the full multi-segment summary: total events, segment count,
   splice validation result, final state hash.

### Single-file mode (existing `valori-verify <file>`)

Unchanged — backward compatible. A single-file verify of a non-genesis segment
prints a warning:
```
⚠️  segment_seq=3 — this is not the genesis segment.
    Splice to predecessor cannot be verified without --log-dir.
    Use --log-dir to verify the full chain across all segments.
```

### Data directory layout assumed

```
data_dir/
  audit/
    events.log              ← active (tail) segment, never compressed
    events.0000.log         ← sealed, may be zstd-compressed (events.0000.log.zst)
    events.0001.log.zst     ← sealed, zstd-compressed
    ...
```

The verifier discovers segments by file name convention (configurable prefix).
This matches the layout Phase 1.8 adopts for the node.

---

## D5 — Transparent zstd Read

Phase 1.8 seals and compresses archived segments. The verifier must read both
`.log` and `.log.zst` files without the caller specifying the format.

### Detection heuristic

```rust
fn maybe_decompress(path: &Path, raw: Vec<u8>) -> Result<Vec<u8>, WireError> {
    // zstd magic: 0xFD 0x2F 0xB5 0x28 (little-endian u32 = 0x28B52FFD)
    if raw.len() >= 4 && raw[0..4] == [0xFD, 0x2F, 0xB5, 0x28] {
        zstd::decode_all(raw.as_slice())
            .map_err(|e| WireError::ZstdDecompress(e.to_string()))
    } else {
        Ok(raw)
    }
}
```

Magic-byte detection is used instead of file extension so that the verifier
works against renamed files and piped stdin without extra flags.

### Dependency

```toml
# valori-verify/Cargo.toml
zstd = { version = "0.13", default-features = false }  # no compression needed on the verify side
```

`default-features = false` disables the `compression` feature — the verifier
only *de*compresses; it never *compresses*. This minimises the attack surface
of the dependency.

### Memory budget

zstd decompresses to a `Vec<u8>` before parsing. Maximum decompressed size is
bounded by:

```rust
const MAX_SEGMENT_DECOMPRESSED_BYTES: usize = 512 * 1024 * 1024; // 512 MiB
```

If the decompressed size exceeds this, the verifier returns:
```
error: decompressed segment would exceed 512 MiB — rejecting to prevent OOM.
       If this is a legitimate segment, raise VALORI_VERIFY_MAX_SEGMENT_MB.
```

The override is an env-var (`VALORI_VERIFY_MAX_SEGMENT_MB`) so it can be
raised for large archival segments without recompilation.

---

## D6 — Verifier Tests

New test file: `crates/valori-verify/tests/hardening.rs`

| Test | What it checks |
|---|---|
| `crafted_large_allocation_rejected` | Encode a fake entry claiming `Vec<u8>` of 1 GiB; verify returns `DecodeLimitExceeded`, no OOM |
| `crafted_zero_dim_header_rejected` | `dim = 0` in header → `InvalidDim` error |
| `crafted_dim_mismatch_rejected` | Header says dim=16, event payload contains dim=32 vector → structural error |
| `multi_segment_splice_valid` | Write 3 segments with correct splices → `verified` |
| `multi_segment_splice_broken` | Tamper with segment 2's `prev_segment_chain_head` → `MISSING OR SUBSTITUTED SEGMENT` |
| `multi_segment_gap_detected` | Delete segment 1, pass segments 0 + 2 → gap detected from splice mismatch |
| `zstd_segment_round_trip` | Write a segment, zstd-compress it, verify reads it correctly |
| `zstd_bomb_rejected` | Craft a valid-magic zstd stream that decompresses to > 512 MiB → rejected |

---

## Implementation Sequence

```
Step 1 — bincode limit in valori-wire (cfg() + new error variant):
  File:  crates/valori-wire/src/lib.rs
  File:  crates/valori-wire/src/lib.rs (WireError::DecodeLimitExceeded)
  Tests: existing wire tests pass; new limit-exceeded test added.

Step 2 — per-field sanity bounds in valori-wire:
  File:  crates/valori-wire/src/lib.rs (parse_header, decode_entry)
  Tests: hardening.rs dim=0, dim-mismatch cases.

Step 3 — zstd dep + transparent decompress in valori-verify:
  File:  crates/valori-verify/Cargo.toml  (add zstd)
  File:  crates/valori-verify/src/main.rs (maybe_decompress wrapper)

Step 4 — multi-segment --log-dir mode:
  File:  crates/valori-verify/src/main.rs  (new Args field, dir_mode fn)
  Tests: hardening.rs multi-segment suite.

Step 5 — valori-fuzz crate skeleton + fuzz targets:
  Files: crates/valori-fuzz/Cargo.toml
         crates/valori-fuzz/fuzz_targets/{fuzz_verify,fuzz_header,fuzz_entry}.rs

Step 6 — CI workflow:
  File:  .github/workflows/fuzz.yml (60-second smoke run on PR)
```

---

## Acceptance Criteria

| Criterion | How verified |
|---|---|
| Crafted 1-GiB-claim entry rejected without OOM | `hardening::crafted_large_allocation_rejected` |
| `dim = 0` header rejected | `hardening::crafted_zero_dim_header_rejected` |
| 3-segment chain with correct splices: `verified` | `hardening::multi_segment_splice_valid` |
| Tampered splice detected as `MISSING OR SUBSTITUTED` | `hardening::multi_segment_splice_broken` |
| zstd segment round-trips correctly | `hardening::zstd_segment_round_trip` |
| zstd bomb rejected at 512 MiB limit | `hardening::zstd_bomb_rejected` |
| 60-second fuzzer CI run: no panics, no OOM | `.github/workflows/fuzz.yml` |
| Full test suite: 0 regressions | `cargo test --workspace` |

---

## Findings

Design-only phase — no runtime findings. One forward concern noted:

**Snapshot verifier gap:** `valori-verify` currently has no way to verify a
snapshot file (`.snap`) against a corresponding event log checkpoint. A
snapshot substitution attack is detectable via the chain-splice mechanism (if
the `restore()` event appears in the log and the snapshot hash in the
`Checkpoint` log entry is checked), but the verifier does not yet cross-
reference these. Deferred to Phase 1.8 (which defines the checkpoint cadence
precisely) — the checkpoint entry format already carries `snapshot_hash`.

**Fuzz corpus quality:** The 60-second CI run is a regression gate, not
sufficient coverage. A dedicated overnight fuzzer schedule (e.g., weekly on
`main`, 10 minutes per target) is needed before Phase 2 ships. Add to the
Phase 2 CI upgrade scope (roadmap § 2.6 testing).

## Follow-ups

- Phase 1.8: wire the `Checkpoint` entry's `snapshot_hash` into the verifier —
  verify that the snapshot at that log height actually hashes to the recorded
  value (requires the verifier to optionally load snapshot files).
- Phase 2: expand fuzz corpus with Raft-era v4 segment fixture logs.
- Phase 2: `VALORI_VERIFY_MAX_SEGMENT_MB` knob documented in deployment runbook.
- Phase 3: audit the snapshot encode/decode path with the same `with_limit`
  discipline (snapshot restore is a trusted path today, but Phase 3 adds
  cloud-downloaded snapshots which are semi-trusted).
