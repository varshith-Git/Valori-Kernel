# operationId Naming & Stability Policy — Valori API v1

Every public operation carries a unique `operationId`. Generated SDKs turn it
into a method name, so it is a published API surface in its own right: renaming
one breaks callers exactly as renaming a path would.

## Where the value comes from

`operation_id = "..."` on the handler's `#[utoipa::path]`. It is never inferred
from the function name or the path, so refactoring Rust internals cannot
silently rename a client method.

`scripts/verify-api-route-contract.py` diffs the operationId three ways — the
route manifest, the utoipa output, and the committed contract — and fails the
gate on any disagreement.

## Rules

1. **`snake_case`**, lowercase, `[a-z][a-z0-9_]*`. All 74 conform.
2. **Unique** across the contract. No duplicates.
3. **`verb_noun`**, verb first, resource singular or plural to match the
   operation's cardinality: `get_record`, `list_collections`.
4. **Standard verbs**: `get_` (fetch one), `list_` (fetch many), `create_`,
   `delete_`, `update_`, `set_` (idempotent replace), `insert_`.
5. **No HTTP method or version in the name** — not `post_records`, not
   `search_v1`. The method lives in the path item; the version in the path.
6. **Domain prefix when the domain is a subsystem**, so related methods sort
   and complete together: `memory_*`, `tree_*`, `community_*`, `graph_*`,
   `cluster_*`.

## Examples from the current contract

| Area | operationIds |
|---|---|
| Collections | `create_collection`, `list_collections`, `delete_collection` |
| Records | `insert_record`, `insert_records_batch`, `get_record`, `delete_record`, `soft_delete_record`, `update_record_metadata` |
| Search | `search`, `search_multi`, `graphrag`, `graph_query` |
| Index | `set_collection_index`, `get_collection_index`, `get_index_config`, `rebuild_indexes` |
| Operations explorer | `list_operations`, `get_operation`, `get_operation_execution` |
| Memory | `memory_upsert`, `memory_search`, `memory_consolidate`, `memory_contradict` |
| Tree-RAG | `tree_build`, `tree_query`, `tree_hybrid`, `tree_verify`, `tree_chain_verify` |
| Proof | `get_state_proof`, `get_event_log_proof`, `get_receipt`, `get_latest_receipt` |

Note `delete_collection`, not `drop_collection` — the Python SDK's method is
`drop_collection()`, but the wire operationId follows rule 4. An earlier draft
of this document listed `drop_collection` as the operationId; that was wrong.

## Stability

An operationId that has appeared in a released `api/openapi/valori-v1.yaml` is
frozen for the life of `v1`. Changing one is a **breaking change** to every
generated SDK and requires a major version bump, not a patch.

Adding a new operation is non-breaking. Removing a public operation, or moving
one out of the public classification, is breaking — see
`docs/phases/phase-api-contract-3.2-readiness.md` for how contract-surface
changes are classified.
