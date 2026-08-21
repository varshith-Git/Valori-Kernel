# Phase 4.2 — Index Lifecycle UI, SDK Integration, and End-to-End Product Experience

## Goal

Expose the backend index lifecycle (introduced in Phase 4 and hardened in Phase 4.1) through the UI and Python SDK. After this phase a user can create, change, and remove an ANN index on any collection directly from the Valori Studio interface, see live build progress, and understand the active-during-build semantics without reading documentation.

## Delivered

### New files

| File | Purpose |
|---|---|
| `ui/studio/src/lib/hooks/useCollectionIndex.ts` | SWR hook for `GET /v1/namespaces/{name}/index`. Polls at 3 s while status is `building` or `ready`; stops for terminal states. |
| `ui/studio/src/components/collections/IndexLifecycleTab.tsx` | Full lifecycle UI: all 5 states (none/building/ready/active/failed), Create/Change/Remove inline panels, cluster-501 detection. |

### Modified files

| File | Change |
|---|---|
| `ui/studio/src/components/tools/ToolsWorkspace.tsx` | Added `IndexLifecycleTab` to `ANALYZE_TABS` as first entry; wired `useCollectionIndex` for live header index display (collection-specific, replaces project-wide `/health` `index` field); "View details" button now navigates to Index tab. |
| `ui/src/components/collections/CollectionList.tsx` | Fixed "BRUTE INDEX" → "No Index" badge for collections without an ANN index (both grid and list view modes). |
| `python/tests/test_index_lifecycle.py` | Extended from 11 → 21 tests: added error scenarios (409, 501, 404) and status model validation (building with active, active, failed with error, none). |
| `crates/valori-engine/src/engine.rs` | `cargo fmt` applied (pre-existing style debt, no logic changes). |
| `crates/valori-engine/src/index_manager.rs` | Same. |

## Findings

1. **BRUTE display bug** — `CollectionList` was showing "BRUTE INDEX" for any collection without an ANN index (`meta?.index` absent or `"brute"`). Fixed by treating absence/brute as "No Index" in both card views.

2. **Project-wide vs collection-specific index** — `CollectionHeader` in ToolsWorkspace was reading `index` from `useHealth()` (a project-wide `/health` field). This showed the node's global default index kind, not the per-collection lifecycle state. Phase 4.2 replaces this with a live call to `GET /v1/namespaces/{name}/index`.

3. **Cloud UI gap** — `CollectionsPanel.tsx` (cloud) has no dedicated collection detail page; it routes to `ToolsWorkspace` via the tools page. Since `IndexLifecycleTab` is now part of ToolsWorkspace's Analyze group, Cloud automatically gets the tab. When cloud cluster nodes respond 501 to POST, the tab shows the backend error message inline.

4. **No backend changes needed** — the Phase 4/4.1 REST contract (`IndexStatusResponse`, `CollectionIndexState`, `IndexSpec`, `IndexBuildRequest`) was complete and correct. Phase 4.2 is purely consumer-side.

## Validation

### Rust (cargo test -p valori-kernel -p valori-node)
```
All test suites: 0 failed
valori-kernel: 1 pass
valori-node: 7 + 12 + 12 + ... passes (full suite green)
cargo fmt --check: clean
cargo clippy --workspace --all-targets --all-features: 0 errors
cargo build -p valori-kernel --target wasm32-unknown-unknown: success
```

### Python SDK (pytest python/tests/test_index_lifecycle.py)
```
21 passed, 0 failed
```
Tests cover:
- URL shape for all 4 methods (sync + async)
- Minimal + parameterized payload shapes
- `change_collection_index` alias delegation
- Error propagation: 409, 501, 404 (sync and async)
- Status model: building with active, active, failed-with-error, none

### TypeScript
```
cd ui/studio && npx tsc --noEmit: 0 errors
cd ui && npx tsc --noEmit: 0 errors
```

### Manual smoke-test (against running node)
```
GET /v1/namespaces/docs/index  → {"collection":"docs","active_type":"none","status":"none"}
POST /v1/namespaces/docs/index {"type":"hnsw"}  → 202 {"status":"building"}
GET /v1/namespaces/docs/index  → {"status":"active","active_type":"hnsw","active_generation":0}
POST /v1/namespaces/docs/index {"type":null}  → 200 {"status":"none"}
```
UI: IndexLifecycleTab renders all states correctly; polling stops on active/failed; "View details" button opens the tab.

## Follow-ups

| Item | Phase |
|---|---|
| Phase 4.3 — Cluster ANN: Raft-replicated collection config + node-local IndexManager | 4.3 |
| Collection list live index status (poll each collection individually) | Optional, perf tradeoff |
| Optimistic UI for status transitions | UX polish |
| Phase 5 — Cross-Collection Query Orchestration | 5 |
