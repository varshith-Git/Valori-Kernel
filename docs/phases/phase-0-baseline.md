# Phase 0 — Baseline: durability, hash chain, offline verifier

**Status:** done · merged to `main` via PR #3 (`57da43e`), June 2026
**Roadmap:** prerequisite work, done before the multi-node phases started

## Goal

Make the core single-node claims actually true before building anything
distributed on top of them: acknowledged writes must survive `kill -9`,
and the event log must be independently verifiable offline.

## Delivered

- **fsync-per-append durability** — `EventLogWriter::append()` writes,
  flushes, and fsyncs before returning. Previously it wrote into a
  `BufWriter` that nothing flushed: a `kill -9` dropped *every*
  acknowledged single-event write while the server returned HTTP 200.
  This directly contradicted the "mathematically proven crash recovery"
  marketing claim.
- **Kill-test** (`crates/valori-node/tests/crash_durability.rs`) — child
  process appends 50 events and `abort()`s; the parent verifies all 50
  survive. Guards the regression forever.
- **Hash-chained log (format v2)** — every entry stores the BLAKE3 chain
  head that preceded it; an in-place edit to entry *i* breaks entry
  *i+1*'s `prev_hash`, localizing tampering to the exact event.
- **`valori-verify` rewrite** — standalone offline verifier for the
  current event-log format (the old binary read a long-dead WAL format).
  Verdicts: `VERIFIED`, `TAMPERED (chain breach at entry #N)` with the
  decoded altered entry and commit timestamp, `TAMPERED (structural)`,
  `TAMPERED (semantic)`, `TAMPERED (content)`. Machine-readable forensic
  reports via `--report`.
- **`make-demo-log` + `tamper_demo.sh`** — the 30-second pitch: generate
  a log, verify it, flip single bytes in two copies, watch both attacks
  get caught with event-level localization.
- **Capacity enforcement** — `VALORI_MAX_RECORDS` / `_NODES` / `_EDGES`
  are hard limits returning HTTP 507; previously inserts could OOM the
  process.
- **Graph cascade tests** — first test coverage for the reverse edge
  index (O(in+out degree) node deletion).

## Findings

1. **The BufWriter flush bug** (fixed above) — discovered when the first
   real-server verification attempt produced a 16-byte log file: all 51
   HTTP-acknowledged events had been lost on `kill -9`.
2. **Wire-format drift #1** — the verify crate's format mirror was one
   version behind what the node wrote (v1 bare entries vs v2 chained).
   Root cause of Phase 1.2's single-definition design.

## Validation

- Real `valori-node`, 50 inserts over HTTP, live `/v1/proof/state` hash
  captured, `kill -9`, offline verify of the log file alone:
  **hash match, VERIFIED** — the full trust loop closed end to end.
- Tamper demo at 2,000 and 50,000 events: pristine verifies, content
  flip and structure flip both caught.

## Follow-ups

- Per-write fsync costs ~1–5 ms/insert on SSDs → group commit lands with
  Phase 2 Raft batching (`measure_append_fsync_throughput` exists,
  `--ignored`, for baselining).
- Segment rotation reset the chain to zeros (whole-segment deletion
  undetectable) → fixed in Phase 1.2.
