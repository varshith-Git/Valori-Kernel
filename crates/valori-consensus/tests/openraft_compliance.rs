// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! Phase 2.3 — the official openraft storage compliance suite, run over
//! `ValoriLogStore` + `ValoriStateMachine` as a pair.
//!
//! This is the same suite openraft's own example stores pass. It exercises
//! the full storage contract: log state reporting, vote persistence,
//! truncation/purge edge cases, apply ordering, snapshot meta consistency.
//! Phase 2.10's redb-backed store must pass this exact suite to land.

use openraft::testing::{StoreBuilder, Suite};
use openraft::StorageError;

use valori_consensus::types::{NodeId, TypeConfig};
use valori_consensus::{ValoriLogStore, ValoriStateMachine};

struct Builder;

impl StoreBuilder<TypeConfig, ValoriLogStore, ValoriStateMachine, ()> for Builder {
    async fn build(
        &self,
    ) -> Result<((), ValoriLogStore, ValoriStateMachine), StorageError<NodeId>> {
        Ok(((), ValoriLogStore::new(), ValoriStateMachine::default()))
    }
}

#[test]
fn openraft_storage_compliance_suite() {
    Suite::test_all(Builder).expect("openraft compliance suite must pass");
}
