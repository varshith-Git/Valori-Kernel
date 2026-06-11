# Phase 1.2 — valori-wire crate + segment format v3

**Status:** done · commit `b4ac53b` on `multinode`
**Roadmap:** [MULTINODE_ROADMAP.md](../MULTINODE_ROADMAP.md) § 1.2 — "the keystone"

## Goal

One definition of the on-disk format (it was defined three times and
drifted twice), plus the single v3 header bump carrying every field that
would otherwise calcify wrong once production logs exist: arithmetic
format ID, segment splicing, and the request-ID envelope Phase 2's Raft
dedup needs.

## Delivered

**`crates/valori-wire`** — `LogEntry`, `EntryV2`/`EntryV3`,
`SegmentHeader`, `parse_header`/`decode_entry`/`encode_entry` with
version dispatch, `chain_advance_v2/v3`. Tiny dependency set (serde,
bincode, blake3, thiserror) to preserve the auditor story. Consumed by:

- `valori-node` — `event_log.rs` (writer), `event_replay.rs` (recovery),
  `replication.rs` (leader stream)
- `valori-verify` — `main.rs`, `make-demo-log`, `valori-anchor`;
  the duplicated `src/wire.rs` is deleted
- `valori-cli` — `engine.rs` (forensic replay), `timeline.rs`

**Format v3** (48-byte header):

| Field | Purpose |
|---|---|
| `format_id` (u8) | arithmetic format, Q16.16 = 1 — the slot Phase 1.3's `FxpFormat` needs; unknown IDs refused loudly |
| `segment_seq` (u32) | 0 = genesis |
| `prev_segment_chain_head` ([u8;32]) | final chain head of the previous segment |

**Segment splicing** — `rotate()` no longer resets the chain to zeros.
The new segment's header binds to the archived segment's final head and
entries continue from it. Closes the gap where an entire archived
segment could be deleted or substituted undetectably.

**Request-ID envelope** — v3 entries carry
`request_id: Option<[u8;16]>`, covered by the chain hash, plumbed via
`EventLogWriter::append_with_request_id()`. Production v3 logs will
never need migrating to gain idempotency.

**Compatibility** — v2 files reopen, append in their own format, and
upgrade to v3 at their next rotation (chain spliced). Every reader
handles both versions.

**Evolution policy** (`crates/valori-wire/README.md`) — variants
append-only; no field changes within a version; readers keep every
shipped version; writers emit only the newest. Enforced by committed
binary fixtures (`tests/fixtures/segment_v{2,3}.bin`) that must decode
with their recorded chain heads forever — regenerating fixtures to
silence a failure is explicitly forbidden.

## Findings

1. **Recovery never validated the hash chain.**
   `test_fail_on_corrupted_middle` had only ever passed by luck: it
   flips one mid-file bit and expects recovery to fail, but
   `read_event_log` accepted any bytes that still *decoded* — silent
   acceptance of tampered data into the recovered state. v3's layout
   shifted the byte positions and exposed it. Recovery now
   chain-validates every entry against the segment's starting head, so
   in-place corruption of any non-final entry is caught at startup
   deterministically.
2. Fixture files initially named `*.log` were silently swallowed by the
   repo's `.gitignore` (`*.log`) — renamed to `*.bin`. Worth remembering
   for any future committed binary fixtures.

## Validation

- Full suite: **156 tests passing, 0 failures** (incl. new wire codec,
  fixture, splice-verification, and v2-legacy tests)
- Tamper demo on v3: pristine verifies; content flip caught at entry
  #1007; structure flip caught at entry #1
- Real server on v3: 30 HTTP inserts, `kill -9`, offline verify —
  hash match, chain intact across all 30 entries

## Follow-ups

- Multi-segment verification (follow splices across archived files) —
  verifier currently validates one segment; the header fields make the
  cross-file walk straightforward. Slated with Phase 1.7/1.8 work.
- `valori-verify` still decodes attacker-controlled input without
  allocation limits → Phase 1.7 (fuzzing + `with_limit`).
