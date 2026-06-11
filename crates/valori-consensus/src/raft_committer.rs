// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! RaftCommitter stub — compile-time proof the Committer trait is satisfiable
//! from valori-consensus.
//!
//! All methods panic. Phase 2 replaces the panic bodies with openraft logic.
//! This file must compile cleanly as part of the Phase 1.9 acceptance gate.
//!
//! See docs/phases/phase-1.9-committer-trait.md for the full design.

// NOTE: valori-node is not yet a dependency of valori-consensus.
// Phase 1.9 adds it (dev-dependency only — consensus never ships inside node).
// This file is a placeholder that will compile once the dep is wired.
//
// Placeholder content so the file exists in the repo and documents intent:

/// Phase 2 Raft committer — stub.
///
/// Replace panic bodies with:
///   openraft::RaftClient::write(event) → await → CommitReceipt { log_index }
pub struct RaftCommitter;

// Phase 1.9: impl Committer for RaftCommitter { … panic!("Phase 2") … }
// Uncomment and fill in once valori-node::commit is a dep of valori-consensus.
