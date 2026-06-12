// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! Phase 2.5 — cluster bootstrap and the RaftCommitter seam.
//!
//! The flagship test boots a real single-node cluster (gRPC server and all),
//! commits events through the `Committer` trait, and then REPLAYS THE AUDIT
//! LOG from disk with the standalone recovery path — proving the cluster
//! writes the exact same chained `events.log` an auditor already knows how
//! to verify.

use std::time::Duration;

use valori_consensus::types::ValoriNode;
use valori_node::cluster::{bootstrap_cluster, ClusterConfig, ClusterConfigError};
use valori_node::commit::{CommitError, Committer, EventLogAuditSink};
use valori_node::events::event_log::EventLogWriter;
use valori_node::events::event_replay::read_event_log;

use valori_kernel::event::KernelEvent;
use valori_kernel::types::id::RecordId;
use valori_kernel::types::vector::FxpVector;

// ── Config parsing (pure, no env mutation) ────────────────────────────────────

#[test]
fn parse_full_topology() {
    let cfg = ClusterConfig::parse(
        2,
        "0.0.0.0:3100",
        "1=10.0.0.1:3100/10.0.0.1:3000, 2=10.0.0.2:3100/10.0.0.2:3000, 3=10.0.0.3:3100",
        false,
    )
    .unwrap();

    assert_eq!(cfg.members.len(), 3);
    assert_eq!(
        cfg.members[&1],
        ValoriNode {
            raft_addr: "10.0.0.1:3100".into(),
            api_addr: "10.0.0.1:3000".into(),
        }
    );
    // api_addr is optional per member.
    assert_eq!(cfg.members[&3].api_addr, "");
}

#[test]
fn parse_rejects_node_not_in_members() {
    let err = ClusterConfig::parse(9, "0.0.0.0:3100", "1=a:1,2=b:2", false).unwrap_err();
    assert_eq!(err, ClusterConfigError::SelfNotInMembers(9));
}

#[test]
fn parse_rejects_malformed_entries() {
    assert!(matches!(
        ClusterConfig::parse(1, "x", "not-an-entry", false),
        Err(ClusterConfigError::BadMemberEntry(_))
    ));
    assert!(matches!(
        ClusterConfig::parse(1, "x", "abc=host:1", false),
        Err(ClusterConfigError::BadMemberId(_))
    ));
    assert!(matches!(
        ClusterConfig::parse(1, "x", "1==/", false),
        Err(ClusterConfigError::BadMemberEntry(_))
    ));
}

// ── Single-node cluster, end to end ───────────────────────────────────────────

fn insert(id: u32) -> KernelEvent {
    KernelEvent::InsertRecord {
        id: RecordId(id),
        vector: FxpVector::new_zeros(4),
        metadata: Some(vec![id as u8]),
        tag: id as u64,
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn raft_committer_writes_a_verifiable_audit_log() {
    let dir = tempfile::tempdir().unwrap();
    let log_path = dir.path().join("events.log");

    // The audit sink is the node's real chained event-log writer.
    let writer = EventLogWriter::open(&log_path, Some(4)).unwrap();
    let audit = EventLogAuditSink::new(writer);

    // Boot a single-node cluster; raft_bind :0 picks a free port.
    let cfg = ClusterConfig {
        node_id: 1,
        raft_bind: "127.0.0.1:0".into(),
        members: [(1, ValoriNode { api_addr: String::new(), raft_addr: String::new() })]
            .into_iter()
            .collect(),
        init: true,
        raft_log_path: None,
        tls: None,
    };
    let handle = bootstrap_cluster(&cfg, Box::new(audit)).await.unwrap();

    handle
        .raft
        .wait(Some(Duration::from_secs(10)))
        .metrics(|m| m.current_leader == Some(1), "self-elected")
        .await
        .unwrap();

    // Commit through the Committer seam — the API the Engine will call.
    let mut committer = handle.committer();
    let r1 = committer.commit(insert(0)).unwrap();
    let r2 = committer.commit(insert(1)).unwrap();
    let r3 = committer
        .commit(KernelEvent::DeleteRecord { id: RecordId(0) })
        .unwrap();
    assert!(r1.log_index < r2.log_index && r2.log_index < r3.log_index);

    // Kernel state reflects the commits.
    assert_eq!(handle.state_machine.with_state(|s| s.record_count()).await, 1);

    // log_height reads Raft metrics, which lag client_write by a hair
    // (the Phase 2.4 finding) — wait for the metrics to catch up.
    handle
        .raft
        .wait(Some(Duration::from_secs(10)))
        .applied_index_at_least(Some(r3.log_index), "metrics caught up")
        .await
        .unwrap();
    assert!(committer.log_height() >= r3.log_index);

    // THE point of the phase: the audit log on disk is a normal chained
    // v3 segment — replayable and chain-checked by the standalone path.
    let events = read_event_log(&log_path, Some(4)).unwrap();
    assert_eq!(
        events.iter().map(|e| e.event_type()).collect::<Vec<_>>(),
        vec!["InsertRecord", "InsertRecord", "DeleteRecord"],
        "audit log holds exactly the committed events, in commit order"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn deterministic_rejection_surfaces_as_rejected_not_io() {
    let cfg = ClusterConfig {
        node_id: 1,
        raft_bind: "127.0.0.1:0".into(),
        members: [(1, ValoriNode { api_addr: String::new(), raft_addr: String::new() })]
            .into_iter()
            .collect(),
        init: true,
        raft_log_path: None,
        tls: None,
    };
    let handle = bootstrap_cluster(&cfg, Box::new(valori_consensus::NullAuditSink))
        .await
        .unwrap();
    handle
        .raft
        .wait(Some(Duration::from_secs(10)))
        .metrics(|m| m.current_leader == Some(1), "self-elected")
        .await
        .unwrap();

    let mut committer = handle.committer();
    // id=7 violates the sequential-id rule — deterministic kernel rejection.
    let err = committer.commit(insert(7)).unwrap_err();
    assert!(
        matches!(err, CommitError::Rejected(_)),
        "kernel rejection must surface as Rejected, got {err:?}"
    );
    assert_eq!(handle.state_machine.with_state(|s| s.record_count()).await, 0);
}

// ── Phase 2.10: crash-restart with the persistent Raft log ───────────────────

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn node_restart_recovers_state_from_the_persistent_raft_log() {
    let dir = tempfile::tempdir().unwrap();
    let raft_log = dir.path().join("raft.redb");

    let cfg = ClusterConfig {
        node_id: 1,
        raft_bind: "127.0.0.1:0".into(),
        members: [(1, ValoriNode { api_addr: String::new(), raft_addr: String::new() })]
            .into_iter()
            .collect(),
        init: true,
        raft_log_path: Some(raft_log.clone()),
        tls: None,
    };

    // ── Life 1: write 5 records, record the hash, then crash ─────────────────
    let hash_before = {
        let handle = bootstrap_cluster(&cfg, Box::new(valori_consensus::NullAuditSink))
            .await
            .unwrap();
        handle
            .raft
            .wait(Some(Duration::from_secs(10)))
            .metrics(|m| m.current_leader == Some(1), "self-elected")
            .await
            .unwrap();

        let mut committer = handle.committer();
        for i in 0..5u32 {
            committer.commit(insert(i)).unwrap();
        }
        let hash = handle.state_machine.state_hash().await;

        // Crash: stop the Raft core and the gRPC server. The kernel state
        // (in-memory) dies with the process; only the redb file survives.
        let _ = handle.raft.shutdown().await;
        handle.server_task.abort();
        hash
    };

    // ── Life 2: same redb file, fresh everything else ────────────────────────
    // init: true is safe — openraft refuses a second initialize and the
    // bootstrap treats that as "fine" (membership is in the log).
    let handle = bootstrap_cluster(&cfg, Box::new(valori_consensus::NullAuditSink))
        .await
        .unwrap();
    handle
        .raft
        .wait(Some(Duration::from_secs(10)))
        .metrics(|m| m.current_leader == Some(1), "re-elected from persisted vote+membership")
        .await
        .unwrap();

    // The state machine starts empty and is rebuilt by replaying the
    // persisted Raft log — the kernel must converge to the pre-crash hash.
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    loop {
        if handle.state_machine.with_state(|s| s.record_count()).await == 5 {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "state machine never rebuilt from the persisted log"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert_eq!(
        handle.state_machine.state_hash().await,
        hash_before,
        "post-restart replay must reproduce the exact pre-crash state hash"
    );

    // And the reborn node keeps accepting writes.
    let mut committer = handle.committer();
    committer.commit(insert(5)).unwrap();
    assert_eq!(handle.state_machine.with_state(|s| s.record_count()).await, 6);
}
