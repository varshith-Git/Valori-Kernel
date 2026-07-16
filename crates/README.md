# Crates

This workspace is split into 15 focused crates. Each has its own README with full details.

## Crate Summary Table

| Crate | One-liner | README | Layer |
|---|---|---|---|
| [`valori-core`](valori-core/) | Zero-dependency `no_std` type foundation (shared IDs, error types, traits) | [→](valori-core/README.md) | Core |
| [`valori-kernel`](valori-kernel/) | Deterministic core — Q16.16 fixed-point vector store, knowledge graph, BLAKE3 audit chain, snapshot encode/decode (`no_std`) | [→](valori-kernel/README.md) | Core |
| [`valori-wire`](valori-wire/) | Shared serialization types + V2/V3/V4 event-log format (encode/decode/chain) | [→](valori-wire/README.md) | Protocol |
| [`valori-storage`](valori-storage/) | Durable storage layer: WAL, append-only event log (V4), object-store backend (S3/file) | [→](valori-storage/README.md) | Storage |
| [`valori-state`](valori-state/) | State lifecycle orchestration: transitions `KernelState` between durable storage and in-memory operation | [→](valori-state/README.md) | State |
| [`valori-metadata`](valori-metadata/) | Control-plane persistence (`redb`): project config, collection name mappings, shard topology, execution history | [→](valori-metadata/README.md) | Control Plane |
| [`valori-planner`](valori-planner/) | Operation lifecycle + execution planning: turns `Operation` + `PlanningContext` into a DAG of `TaskSpec`s | [→](valori-planner/README.md) | Control Plane |
| [`valori-effect`](valori-effect/) | Effect system: routes kernel writes, receipt fragments, audit entries, and metrics from task execution | [→](valori-effect/README.md) | Execution |
| [`valori-consensus`](valori-consensus/) | Raft state machine + log store (`openraft 0.9`). Wraps the kernel as a multi-shard `RaftStateMachine` | [→](valori-consensus/README.md) | Consensus |
| [`valori-engine`](valori-engine/) | Stateful engine orchestrator — `Engine` struct, `EngineConfig`, persistence funnel, metadata sidecar | [→](valori-engine/README.md) | Engine |
| [`valori-node`](valori-node/) | HTTP server (`axum`) + cluster orchestration; constructs `EngineConfig` from env and injects vault | [→](valori-node/README.md) | Server |
| [`valori-cli`](valori-cli/) | `valori` CLI binary — `setup` wizard, `cluster`, `inspect`, `verify`, `timeline`, `diff`, `import` | [→](valori-cli/README.md) | Tools / CLI |
| [`valori-ffi`](valori-ffi/) | PyO3 FFI layer — embedded in-process Python SDK (`MemoryClient`) | [→](valori-ffi/README.md) | SDK / FFI |
| [`valori-mcp`](valori-mcp/) | Model Context Protocol server (`stdio`) — verifiable agent memory with BLAKE3 receipts | [→](valori-mcp/README.md) | Integration |
| [`valori-verify`](valori-verify/) | Standalone offline verifier — replays `events.log` and checks the BLAKE3 chain without a server | [→](valori-verify/README.md) | Tools / Verification |

## Architectural Dependency Flow

```
valori-core (no_std)
   └── valori-kernel (no_std)
          └── valori-wire
                 ├── valori-storage
                 │      └── valori-state
                 ├── valori-metadata
                 │      └── valori-planner
                 │             └── valori-effect
                 └── valori-node ── (uses consensus, state, planner, effect)
```

