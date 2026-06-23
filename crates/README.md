# Crates

This workspace is split into focused crates. Each has its own README with full details.

| Crate | One-liner | README |
|---|---|---|
| [`valori-kernel`](valori-kernel/) | Deterministic core — Q16.16 fixed-point vector store, knowledge graph, BLAKE3 audit chain, snapshot encode/decode | [→](valori-kernel/README.md) |
| [`valori-node`](valori-node/) | HTTP server (axum) + standalone engine + cluster orchestration | [→](valori-node/README.md) |
| [`valori-consensus`](valori-consensus/) | Raft state machine + log store (openraft 0.9). Wraps the kernel as a `RaftStateMachine` | [→](valori-consensus/README.md) |
| [`valori-mcp`](valori-mcp/) | Model Context Protocol server — verifiable agent memory with BLAKE3 receipts | [→](valori-mcp/README.md) |
| [`valori-cli`](valori-cli/) | `valori` binary — `setup` wizard, `inspect`, `verify`, `timeline`, `diff`, `import` | [→](valori-cli/README.md) |
| [`valori-ffi`](valori-ffi/) | PyO3 FFI layer — embedded in-process Python SDK (`MemoryClient`) | [→](valori-ffi/README.md) |
| [`valori-wire`](valori-wire/) | Shared serialization types used by node ↔ Python SDK ↔ CLI | [→](valori-wire/README.md) |
| [`valori-verify`](valori-verify/) | Standalone offline verifier — replays `events.log` and checks the BLAKE3 chain without a server | [→](valori-verify/README.md) |
