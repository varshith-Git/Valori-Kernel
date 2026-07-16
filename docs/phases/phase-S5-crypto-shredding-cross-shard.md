# Phase S5 — Crypto-shredding cross-shard safety

Branch: `Node-scaleup` (S1 `6d53924`, S2 `08dd043`, S3 `0460cee`, S4 `809b87a` merged).

## Goal

S3/S4 left crypto-shredding (`cluster_insert_encrypted`, `cluster_shred_key`,
`cluster_crypto_status`) unrouted, flagged as needing "a real cross-shard
fan-out design" before it could safely participate in namespace-correct
sharding. This phase designs and ships that fan-out, so encrypted records
route to the correct shard like every other write, and GDPR erasure
(`DELETE /v1/crypto/shred/:key_id`) still reaches every copy of a key's
ciphertext regardless of which shard(s) it landed on.

## Delivered

### `cluster_insert_encrypted` — routes like any other write

Resolves `payload.collection` to `ns_id`, then submits
`AutoInsertRecordEncrypted` through `state.shard_for(ns_id).raft` with
`namespace_id: ns_id` — identical pattern to every S3b/S4 write handler.
One consequence, called out explicitly: a single `key_id` can now
legitimately have ciphertext on **multiple different shards** if the same
key was used to encrypt inserts into more than one collection. This is not
a bug — it mirrors how any other identifier (not just crypto keys) is
already shard-scattered by collection — but it's the reason `shred_key`
needed the redesign below rather than a single-shard fix.

### `cluster_shred_key` — fan-out, not a single routed write

The vault DEK (`AesGcmVault`, per-node, local) is destroyed **first and
unconditionally** — this is the actual compliance-critical, irreversible
step; it never depends on shard topology. Then the handler loops over
every shard this node runs (`state.shards.iter()`) and submits
`KernelEvent::ShredKey { key_id }` to each one's Raft group independently,
aggregating per-shard outcomes into the response body:

```json
{
  "key_id": "<hex>",
  "shredded": true,
  "shards": {
    "shard_0": {"status": "shredded"},
    "shard_1": {"status": "shredded"},
    "shard_2": {"status": "not-leader", "leader_api_addr": "..."}
  }
}
```

`shredded` is `true` only when every shard reports `"shredded"`. A
`"not-leader"` status is not a failure — it means this node isn't the
leader for that particular shard's Raft group, and the caller should retry
the same `DELETE` (idempotent: `KernelState::apply_shred_key` is a no-op on
a shard holding no matching records for that `key_id`, so re-attempting
already-completed shards costs nothing and is always correct).

This deliberately avoids building a distributed scatter-gather proxy (one
node routing writes to leaders it doesn't own) — the caller/SDK retries
against the same endpoint until `shredded: true`, which is simpler,
observable, and doesn't add a new distributed-transaction failure mode to
an operation whose core safety property (DEK destruction) already doesn't
need one.

### `cluster_crypto_status` — unchanged

Reads only the per-node vault's key existence (`{"exists": bool}`), which
was never namespace- or shard-scoped — no change needed.

## Findings

- Crypto-shredding's true correctness boundary is the **vault**, not
  `KernelState`. `FLAG_SHREDDED` on records is an audit/visibility flag,
  not the erasure mechanism itself — the DEK is genuinely gone the moment
  `vault.shred()` returns, regardless of how many shards still show
  `FLAG_SHREDDED` unset. This matters for the phase doc record: even if a
  shard is temporarily unreachable and its records never get flagged, the
  ciphertext under that key is still cryptographically unrecoverable.
- Encrypted inserts require the target shard's kernel `dim` to already be
  locked by a prior plain insert — a pre-existing kernel constraint
  (`InsertRecordEncrypted` does `self.dim.ok_or(InvalidOperation)?`), not
  new to this phase, but it surfaced while writing the cross-shard test
  (had to insert a plain vector into each tenant's collection before the
  encrypted insert would succeed there).

## Validation

```
cargo build -p valori-kernel --target wasm32-unknown-unknown   # clean (untouched)
cargo test -p valori-kernel        # 62 passed
cargo test -p valori-consensus     # 74 passed, 1 ignored (pre-existing)
cargo test -p valori-node          # 235 passed
cargo test -p valori-cli           # 11 passed
```

New test:
`crates/valori-node/tests/cluster_namespaces.rs::shred_key_reaches_records_on_every_shard`
— inserts the same `key_id`'s ciphertext into `tenant-a` and `tenant-b`
(two different shards under `shard_count=3`), shreds once, and asserts
`FLAG_SHREDDED` (`0x04`) is set on both records via
`KernelState::get_record(...).flags`.

## Follow-ups

- If a shard is unreachable for an extended period (not just a transient
  "not-leader"), `shredded` stays `false` indefinitely from the caller's
  view even though the vault DEK is already gone — worth a monitoring/alert
  hook in a future phase so "still not fully propagated" doesn't go
  unnoticed silently.
- Composite external record/node/edge ids remain per-shard-unique only —
  unchanged, tracked in the S3/S4 follow-up list, closed out with explicit
  scope reasoning in the final S5-S9 regression phase doc.
