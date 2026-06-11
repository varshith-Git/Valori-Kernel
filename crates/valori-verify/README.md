# valori-verify

Standalone offline verifier for Valori event logs. **No server. No network. No trust required.**

This is the executable form of the project's core claim: because the kernel is a
deterministic, float-free state machine, *anyone* can replay an event log and
recompute the exact BLAKE3 state hash a live server reports at
`GET /v1/proof/state`. If a single byte of history was altered, verification
fails — and the verifier tells you **which event** was tampered with.

## Usage

```bash
# Audit mode — replay, validate the hash chain, print the state hash
valori-verify events.log

# Verification mode — compare against a known hash
valori-verify events.log --expected-hash 2dfad476977709f3...

# Forensics — also write a machine-readable JSON report
valori-verify events.log --expected-hash <HEX> --report report.json
```

| Verdict | Meaning | Exit code |
|---|---|---|
| `VERIFIED` | Chain intact, full replay succeeded, hash matches | 0 |
| `TAMPERED (chain breach at entry #N)` | An entry's stored `prev_hash` doesn't match the recomputed chain — the prior entry was altered in place; reports the altered entry's decoded content and byte offset | 1 |
| `TAMPERED (structural)` | An entry failed to decode — reports the event number and byte offset of the damage | 1 |
| `TAMPERED (semantic)` | An entry decoded but the kernel rejected it | 1 |
| `TAMPERED (content)` | Chain and replay are clean but the final state hash differs — a sophisticated attacker rewrote history *and* recomputed every chain hash; only the expected hash catches this | 1 |

## Two layers of defense

The log format (v2) chains every entry:

```
chain_hash[i] = BLAKE3(chain_hash[i-1] || bincode((wall_time_secs_i, entry_i)))
```

1. **The hash chain** catches in-place edits *without any external information*
   and localizes them to the exact event — including when it happened
   (each entry carries its commit timestamp).
2. **The state hash** (`--expected-hash`, from `/v1/proof/state`) catches even
   an attacker who rewrote the log and recomputed the entire chain — they
   cannot produce a different history with the same final state hash.

## The 30-second demo

```bash
./verify/tamper_demo.sh            # default 2000 events
./verify/tamper_demo.sh 50000      # bigger log
```

Generates a log, verifies it (`VERIFIED`), then runs two attacks against
copies of the file:

1. **Content attack** — flip one byte inside a stored vector. The chain
   breaks at the *next* entry, and the verifier prints the altered event,
   its decoded contents, its commit time, and a forensic JSON report.
2. **Structure attack** — flip one byte in an entry frame. The verifier
   names the exact event where history stops being trustworthy.

## Verifying a real server

```bash
# On the live server
HASH=$(curl -s $VALORI_URL/v1/proof/state \
  | python3 -c "import json,sys; print(''.join(f'{b:02x}' for b in json.load(sys.stdin)['final_state_hash']))")

# Anywhere else, with a copy of the log file
valori-verify events.log --expected-hash $HASH
```

This exact flow is validated end-to-end — including crash durability: a
production `valori-node` accepts 50 inserts over HTTP, is killed with
`kill -9` (no graceful shutdown, no flush), and the standalone verifier
replays the log file alone:

```
replayed:   50 events, 0 checkpoints
state hash: 2dfad476...  (matches the live server's /v1/proof/state)
✅  VERIFIED — hash chain intact across all 50 entries.
```

Every event acknowledged with HTTP 200 is fsync'd to disk before the
response is sent (see `node/src/events/event_log.rs` and the kill-test in
`node/tests/crash_durability.rs`).

## Design notes

- The wire format (`src/wire.rs`) mirrors `node/src/events/event_log.rs`
  **on purpose** — the verifier must not depend on the server crate, so an
  auditor reads ~400 lines of code instead of trusting a database binary.
  Any format change requires a header version bump, which this parser
  rejects loudly.
- The state hash is `valori_kernel::snapshot::blake3::hash_state_blake3`,
  the same function behind `/v1/proof/state`. One hash, one truth.
- `make-demo-log` generates deterministic logs (fixed-seed LCG, fixed
  timestamps) in the production wire format, so demo runs are reproducible
  bit-for-bit.
