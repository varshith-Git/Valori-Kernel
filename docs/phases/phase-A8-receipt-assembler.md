# Phase A8 — ReceiptAssembler + `/v1/proof/receipt`

## Goal

Introduce a unified, self-describing `Receipt` type that replaces three ad-hoc proof
structures (`EventProof`, per-recall MCP receipt, Tree-RAG citation chain). Every completed
operation now produces one `Receipt` carrying a BLAKE3-content-addressed, offline-verifiable
proof of what ran and what state changed. Expose it via `GET /v1/proof/receipt` (latest) and
`GET /v1/proof/receipt/:id` (by id) on both standalone and cluster routers.

## Delivered

### `crates/valori-effect/src/receipt.rs` (new)

| Symbol | Description |
|---|---|
| `ReceiptHash([u8; 32])` | `BLAKE3(op_hash ‖ graph_hash ‖ state_before ‖ state_after ‖ sorted(parent_hashes) ‖ shard_id ‖ committed_height)` — `produced_at` excluded intentionally |
| `StateHash(String)` | Opaque BLAKE3 hex of a kernel state snapshot |
| `Receipt` | Full proof: identity, what ran, under what contract, state transition, Merkle DAG, provenance |
| `ReceiptEnvelope` | Versioned outer wrapper (`version: u8`, `payload: Receipt`) |
| `ReceiptAssembler` | Collects `ReceiptFragment`s from `EffectBus`, sorts by `task_index`, assembles the final `Receipt` with correct `state_before/after` chain |
| `verify_receipt(receipt)` | Step 1: recompute hash; Step 2: verify fragment state chain; Step 3: outer hash consistency |
| `ReceiptStore { capacity }` | In-process last-N cache backed by `Mutex<HashMap>` + insertion-order `Vec`; `insert/get/latest/list_ids` |

### `crates/valori-effect/src/lib.rs`

Added `pub mod receipt` and re-exported all public symbols from `receipt.rs`.

### `crates/valori-node/src/server.rs`

- Added routes: `GET /v1/proof/receipt` (`get_latest_receipt`) and `GET /v1/proof/receipt/:id` (`get_receipt_by_id`).
- `build_router_with_keys` now accepts `Arc<ReceiptStore>` as fifth parameter; injects it via `.layer(Extension(receipt_store))`.
- `build_router` wrapper creates a `ReceiptStore::new(256)` and forwards it.

### `crates/valori-node/src/cluster_server.rs`

- Added the same two routes to the v1 block.
- `build_cluster_router_with_keys` accepts `Arc<ReceiptStore>` as sixth parameter; injects via `.layer(Extension(receipt_store))`.
- `build_cluster_router` creates `ReceiptStore::new(256)` and forwards it.
- Handler functions `cluster_get_latest_receipt` / `cluster_get_receipt_by_id` added.

### `crates/valori-node/src/main.rs`

Creates `ReceiptStore::new(256)` and passes it into `build_router_with_keys`.

### Test files updated

- `tests/api_keys.rs` — added `ReceiptStore::new(64)` argument to `build_router_with_keys` call.
- `tests/cluster_namespaces.rs` — added `ReceiptStore::new(64)` argument to `build_cluster_router_with_keys` call.

## Findings

- `ReceiptFragment.task_index` field name had been coded as `topological_index` in an early draft — caught and fixed before tests ran.
- `produced_at` must be excluded from `receipt_hash` to keep the hash deterministic across serialisation timing jitter (RFC-0003 §4).
- `ReceiptStore` must be injected **after** auth middleware in both routers, not before — otherwise the auth middleware extraction order would break. Applied last via `.layer(Extension(...))`.

## Validation

```
cargo test -p valori-effect
```

16 tests, 0 failures:

- `empty_assembler_produces_zero_hashes`
- `read_only_receipt_has_equal_state_hashes`
- `mutating_receipt_updates_state_after`
- `verify_receipt_passes_for_valid_receipt`
- `verify_receipt_fails_for_tampered_hash`
- `receipt_store_evicts_oldest`
- `receipt_hash_is_deterministic`
- (plus 9 pre-existing bus/capability/task tests)

```
cargo test -p valori-kernel -p valori-node
```

All suites pass; 0 failures across all test binaries.

## Follow-ups

- **Durable receipt log** — `ReceiptStore` is in-process only; a future phase should persist receipts to redb or the object store so they survive node restart.
- **EffectBus → ReceiptAssembler wiring** — `ReceiptAssembler` exists and is tested, but the `EffectBus` does not yet forward `EffectPayload::Receipt` fragments into an assembler automatically. Phase A10 should close this gap.
- **Cluster receipt propagation** — in cluster mode receipts are assembled per-leader; followers do not currently receive them. Future work.
