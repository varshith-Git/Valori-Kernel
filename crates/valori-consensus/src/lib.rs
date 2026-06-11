// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! Raft consensus layer for Valori cluster mode.
//!
//! Phase 2 of the multi-node roadmap (see `docs/MULTINODE_ROADMAP.md`):
//! openraft log storage over the chained event log, kernel state-machine
//! adapter, tonic transport, and the raft/audit storage split.
//!
//! This crate is intentionally empty in Phase 1 — it exists so the
//! workspace layout, CI wiring, and feature flags are settled before any
//! consensus code lands.
