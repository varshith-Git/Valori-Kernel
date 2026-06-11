# valori-wire

The single source of truth for Valori's event-log on-disk format, consumed by:

- `valori-node` — writes the log (recovery and replication read it back)
- `valori-verify` — replays and audits logs offline
- `valori-cli` — forensic timeline / replay / diff

Before this crate existed the format was defined three times and drifted
twice (verify missed v1→v2; the CLI silently read garbage). Every layout
change now happens here, behind a segment-version bump, with committed
fixtures guarding compatibility.

## Format summary

```
v2 (legacy): [16-byte header][EntryV2]...
v3:          [48-byte header][EntryV3]...
```

| v3 header field | Size | Purpose |
|---|---|---|
| `version` | u32 LE | = 3 |
| `dim` | u32 LE | embedding dimension |
| `format_id` | u8 | arithmetic format (1 = Q16.16) — hash-domain relevant |
| reserved | 3 bytes | zero |
| `segment_seq` | u32 LE | 0 = genesis segment |
| `prev_segment_chain_head` | 32 bytes | final chain head of the previous segment |

v3 entries add `request_id: Option<[u8;16]>` — the client idempotency token
that Phase 2's Raft dedup is keyed on.

## The chain, across segments

```
v2: chain[i] = BLAKE3(chain[i-1] || bincode((wall_time, entry)))
v3: chain[i] = BLAKE3(chain[i-1] || bincode((wall_time, request_id, entry)))
```

In v3 the chain **continues across rotations**: a new segment opens at the
archived segment's final head, recorded in its header. Deleting or
substituting an entire archived segment breaks the splice — in v2, every
segment restarted from zeros and whole-segment removal was undetectable.

## Evolution policy

1. **Enum variants are append-only.** bincode encodes variants by index;
   reordering or removing a `LogEntry`/`KernelEvent` variant corrupts every
   existing log. New variants go at the end, gated by a version bump if
   older readers must reject them.
2. **No field changes within a version.** Any shape change = new segment
   version + new entry struct here.
3. **Readers keep every shipped version readable.** vN tooling reads vN−1.
   The committed fixtures under `tests/fixtures/*.bin` decode in CI forever;
   if a refactor breaks them, the refactor is wrong — never "fix" the test
   by regenerating fixtures.
4. **Writers emit only the newest version** for new files. Existing
   older-version files keep appending their own format; rotation upgrades
   the live segment (and splices the chain).

## Auditor notes

This crate plus `valori-kernel` is everything needed to decode and verify a
Valori log — no server code, no async runtime. The dependency set is kept
deliberately tiny (serde, bincode, blake3, thiserror) so it stays readable
in one sitting.
