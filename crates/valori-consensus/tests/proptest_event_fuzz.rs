// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! Proptest-driven event-sequence fuzz for the Raft cluster (Phase 2 roadmap 2.6).
//!
//! Strategy: generate an arbitrary sequence of insert/soft-delete operations,
//! apply them through a 3-node in-memory cluster (partition harness), and
//! assert that every node converges to the same BLAKE3 state hash.
//!
//! This differs from the deterministic tests in `fault_tolerance.rs` in that
//! proptest explores the space of sequences automatically, shrinks failing
//! cases, and persists failures in a local `.proptest-regressions` file.

use proptest::prelude::*;
use tokio::runtime::Runtime;
use valori_consensus::{
    partition_harness::{insert_vector, make_cluster, wait_for_convergence, wait_for_leader},
    ClientRequest,
};
use valori_kernel::{
    event::KernelEvent,
    types::{id::RecordId, scalar::FxpScalar},
};

// ── Operation enum ────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
enum Op {
    /// Insert a vector of length `dim` with a fixed value per component.
    Insert { value: i16 },
    /// Soft-delete the record at the given slot (modulo current record count).
    SoftDelete { slot: u8 },
    /// Hard-delete the record at the given slot (modulo current record count).
    Delete { slot: u8 },
}

// ── Proptest strategy for Op ──────────────────────────────────────────────────

fn arb_op() -> impl Strategy<Value = Op> {
    prop_oneof![
        // 60 % inserts — we need records before we can delete them
        3 => any::<i16>().prop_map(|value| Op::Insert { value }),
        1 => any::<u8>().prop_map(|slot| Op::SoftDelete { slot }),
        1 => any::<u8>().prop_map(|slot| Op::Delete { slot }),
    ]
}

fn arb_ops() -> impl Strategy<Value = Vec<Op>> {
    // 4–20 operations per test case — enough depth to exercise ordering
    // without making each test case too slow (each op is a Raft round trip).
    prop::collection::vec(arb_op(), 4..20)
}

// ── Runner ────────────────────────────────────────────────────────────────────

/// Execute an operation sequence on a 3-node in-memory cluster and verify
/// convergence. Returns `Ok(())` on success, `Err(...)` on assertion failure.
async fn run_ops(ops: Vec<Op>) {
    const DIM: usize = 4;

    let (rafts, sms, _partition) = make_cluster(3).await;
    let (leader_idx, _leader_id) = wait_for_leader(&rafts).await;
    let leader = &rafts[leader_idx];

    let mut inserted_ids: Vec<u32> = Vec::new();

    for op in ops {
        match op {
            Op::Insert { value } => {
                let vec: Vec<FxpScalar> = (0..DIM)
                    .map(|_| FxpScalar(value as i32 * 256)) // scale to fxp
                    .collect();
                let id = insert_vector(leader, vec).await;
                inserted_ids.push(id);
            }
            Op::SoftDelete { slot } => {
                if inserted_ids.is_empty() {
                    continue;
                }
                let idx = (slot as usize) % inserted_ids.len();
                let id = inserted_ids[idx];
                leader
                    .client_write(ClientRequest {
                        event: KernelEvent::SoftDeleteRecord {
                            id: RecordId(id),
                        },
                        request_id: None,
                        schema_version: 0,
                    })
                    .await
                    .ok(); // ignore not-found — id may be already deleted
            }
            Op::Delete { slot } => {
                if inserted_ids.is_empty() {
                    continue;
                }
                let idx = (slot as usize) % inserted_ids.len();
                let id = inserted_ids.remove(idx);
                leader
                    .client_write(ClientRequest {
                        event: KernelEvent::DeleteRecord {
                            id: RecordId(id),
                        },
                        request_id: None,
                        schema_version: 0,
                    })
                    .await
                    .ok();
            }
        }
    }

    // Give every follower time to apply the last committed entry.
    wait_for_convergence(&sms).await;

    // All three nodes must agree on the BLAKE3 state hash.
    let hashes: Vec<_> = {
        let mut v = Vec::new();
        for sm in &sms {
            v.push(sm.state_hash().await);
        }
        v
    };
    assert!(
        hashes.windows(2).all(|w| w[0] == w[1]),
        "state hashes diverged after operation sequence:\nhashes={hashes:?}"
    );
}

// ── Proptest macro bridge ─────────────────────────────────────────────────────

proptest! {
    // 32 cases is fast enough for CI (each case boots a 3-node cluster).
    #![proptest_config(ProptestConfig::with_cases(32))]

    #[test]
    fn prop_event_sequence_converges(ops in arb_ops()) {
        // proptest runs sync closures; we build a fresh single-threaded
        // runtime per test case so harness timers work correctly.
        let rt = Runtime::new().expect("tokio runtime");
        rt.block_on(run_ops(ops));
    }
}
