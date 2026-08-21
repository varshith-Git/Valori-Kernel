// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! G1.2 — `query_graph` result identity across a real engine restart.
//!
//! G1.1 already proved replay- and snapshot-decode-level equivalence at the
//! `valori-rag`/`KernelState` layer. This is the one item from G1.2's
//! required test list that layer can't reach: a full `Engine::try_recover()`
//! cycle (event log → fresh `Engine` → recovered `KernelState`), matching
//! the existing `e2e_recovery.rs` durability-contract pattern.

use tempfile::tempdir;
use valori_kernel::types::enums::{EdgeKind, NodeKind};
use valori_kernel::types::id::NodeId;
use valori_node::config::{IndexKind, NodeConfig};
use valori_node::engine::{Engine, RecoveryMode};
use valori_node::EngineFromNodeConfig;
use valori_rag::graph::{query_graph, Direction, GraphQuery};

fn make_cfg(dir: &std::path::Path) -> NodeConfig {
    let mut cfg = NodeConfig::default();
    cfg.max_records = 64;
    cfg.max_nodes = 64;
    cfg.max_edges = 64;
    cfg.event_log_path = Some(dir.join("events.log"));
    cfg.snapshot_path = Some(dir.join("snapshot.bin"));
    cfg.wal_path = None;
    cfg
}

fn query(n: u32) -> GraphQuery {
    GraphQuery {
        start: NodeId(n),
        direction: Direction::Both,
        edge_kind: None,
        node_kind: None,
        max_depth: 4,
        limit: 1000,
    }
}

#[test]
fn query_graph_result_survives_engine_restart() {
    let dir = tempdir().unwrap();
    let cfg = make_cfg(dir.path());

    let pre_crash_result;
    let a; // node ids, so the post-recovery query targets the same nodes

    {
        let mut engine = Engine::new(&cfg);
        assert_eq!(engine.try_recover(), RecoveryMode::Fresh);

        a = engine
            .create_node_for_record(None, NodeKind::Document as u8, 0)
            .unwrap();
        let b = engine
            .create_node_for_record(None, NodeKind::Chunk as u8, 0)
            .unwrap();
        let c = engine
            .create_node_for_record(None, NodeKind::Concept as u8, 0)
            .unwrap();
        engine.create_edge(a, b, EdgeKind::ParentOf as u8).unwrap();
        engine.create_edge(b, c, EdgeKind::RefersTo as u8).unwrap();
        engine.create_edge(a, a, EdgeKind::Relation as u8).unwrap(); // self-loop

        pre_crash_result = query_graph(engine.kernel_state(), 0, &query(a)).unwrap();
        assert!(
            !pre_crash_result.is_empty(),
            "sanity: the fixture must actually produce a nontrivial result"
        );
        engine.flush_pending_events().unwrap();
        // engine dropped here — simulates a crash, matching e2e_recovery.rs's convention
    }

    let post_recovery_result = {
        let mut engine2 = Engine::new(&cfg);
        let mode = engine2.try_recover();
        assert!(
            matches!(mode, RecoveryMode::EventLog(n) if n > 0),
            "must recover from the event log, got {mode:?}"
        );
        query_graph(engine2.kernel_state(), 0, &query(a)).unwrap()
    };

    assert_eq!(
        pre_crash_result, post_recovery_result,
        "query_graph must return the identical result after a real restart"
    );
}
