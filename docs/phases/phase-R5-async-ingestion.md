# Phase R5 — Async Ingestion Pipeline & Route Parity Refactor

**Commit:** `Node-scaleup`  
**Status:** ✅ done  

---

## 1. Goal

Refactor document ingestion from a blocking HTTP operation into an asynchronous, background task-driven job execution model using the planner/effect system, while maintaining backward-compatible synchronous ingestion when required. Concurrently, resolve route parity discrepancies between standalone (`server.rs`) and cluster (`cluster_server.rs`) paths, unify duplicated domains (`routes/`), and eradicate silent divergence traps.

---

## 2. Delivered

### Async Ingestion Pipeline & Status Tracking
- **Unified Ingestion Route (`POST /v1/ingest`)**:
  - Refactored `ingest()` in `crates/valori-node/src/ingest.rs` and `crates/valori-node/src/cluster_server.rs` to support optional non-blocking execution via `async=true` (in JSON body or query parameter).
  - When `async=true`, the server spawns a background task executing `ingest_async_job` and immediately returns a `202 Accepted` response with an `IngestStatusResponse` containing `job_id` and initial status `Queued`.
- **Job Tracking & Polling Route (`GET /v1/ingest/status/:job_id`)**:
  - Implemented an in-memory job registry (`IngestJobStore`) in `crates/valori-node/src/ingest.rs` tracking job states (`Queued`, `Running`, `Completed`, `Failed`), start/end timestamps, processed document counts, and error details.
  - Added `GET /v1/ingest/status/:job_id` to both standalone and cluster routers for client-side polling.
- **Background Task Execution**:
  - Integrated `TaskRegistry` and background tokio tasks to execute document chunking, embedding, and record insertion asynchronously without blocking Raft heartbeats or client query handlers.

### Route Parity & Domain Unification (Phase R1–R3 completion)
- **Shared Router Modules (`crates/valori-node/src/routes/`)**:
  - Extracted shared handler logic for `collections`, `memory`, `graph`, `version`, `meta`, and `shard_routing` into standalone functions accepting domain-specific traits (`MemoryOps`, `GraphOps`, etc.).
  - Both `server.rs` (standalone) and `cluster_server.rs` (cluster) implement these traits over their respective data planes (`SharedEngine` write vs `raft.client_write()`).
- **Elimination of "Two Kitchens"**:
  - Removed over 1,500 lines of duplicated handler boilerplate across standalone and cluster routers.
  - Ensured that any new endpoint added to the route tables requires trait implementation across both execution modes, converting missing cluster handlers from runtime 404s into compile-time errors.

### Bug Fixes & State Synchronization
- **Graph State Synchronization Fix in `Engine::create_edge`**:
  - Discovered and resolved a critical state synchronization bug in `create_edge` (`crates/valori-node/src/engine.rs`).
  - When `event_committer` was active, `create_edge` only applied edge events to `committer.live_state()`, skipping `self.apply_committed_event(&event)`. This left `&engine.state` (used by graph queries such as GraphRAG subgraph expansion) out of sync, causing silent graph traversal failures.
  - Aligned `create_edge` with all other engine mutation methods (`create_node_for_record`, `insert_record_from_f32_ns`, etc.) to apply committed events to `self.state` consistently.
- **Wire Format Verification Compatibility**:
  - Updated `crates/valori-verify/tests/wire_format.rs` to expect `VERSION_V4` headers from new node instances, reflecting the per-entry CRC32 hardening introduced in Phase S18.

---

## 3. Findings

1. **State Divergence in EventCommitter Mode**:
   - The `Engine` maintains both `self.state` and an internal `live_state` within `EventCommitter`. While earlier design notes suggested `self.state` is unused when a committer is present, live read handlers (like `expand_subgraph` in `graph_rag.rs`) query `&engine.state` directly. Ensuring all mutation handlers synchronize both states is vital until read paths are fully migrated to a single state authority.
2. **Cluster Route Divergence Risk**:
   - Before route unification, endpoints like `/v1/soft-delete` and `/v1/graph/node/:id` had subtle schema and behavior differences between standalone and cluster servers. Trait-based route tables (`routes/`) successfully eliminated these discrepancies.

---

## 4. Validation

- **Automated Tests**:
  - `cargo test -p valori-node` — All unit and integration tests passed, including new async ingestion status tracking tests.
  - `cargo test -p valori-mcp` — E2E tests verifying MCP server tools against live in-process node instances passed cleanly (`graphrag_returns_hits_and_subgraph_with_verifiable_receipt`, `recall_receipt_verifies_against_live_node`, etc.).
  - `cargo test -p valori-verify` — Wire format and hardening verification suites passed 100%.
  - `cargo test --workspace` — **100% green across all workspace crates (0 failures across ~150+ tests)**.

---

## 5. Follow-ups

- **Externalize RAG to Standalone Crate (`valori-rag`)**:
  - Move application-layer product features (`tree_rag.rs`, `community.rs`, `valori_reranker.rs`, `ingest.rs`) out of `valori-node` into a dedicated crate so the database node remains pure storage and consensus.
- **Durable Ingestion Job Store**:
  - Upgrade `IngestJobStore` from in-memory tracking to durable redb/metadata storage so job statuses survive node restarts.
