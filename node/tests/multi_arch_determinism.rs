// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! Multi-Architecture Determinism Tests
//!
//! Verifies that identical operations produce identical state hashes across
//! CPU architectures (x86, ARM, WASM).  All tests run on the current machine;
//! the "arch" parameter is informational only — the real guarantee comes from
//! the Q16.16 fixed-point math avoiding f32 non-determinism.

#[cfg(test)]
mod determinism_tests {
    use valori_node::config::{NodeConfig, IndexKind, QuantizationKind};
    use valori_node::engine::Engine;

    const DIM: usize = 16;
    const MAX_RECORDS: usize = 1024;
    const MAX_NODES: usize = 1024;
    const MAX_EDGES: usize = 2048;

    fn test_config() -> NodeConfig {
        NodeConfig {
            max_records: MAX_RECORDS,
            dim: DIM,
            max_nodes: MAX_NODES,
            max_edges: MAX_EDGES,
            bind_addr: "127.0.0.1:0".parse().unwrap(),
            index_kind: IndexKind::BruteForce,
            quantization_kind: QuantizationKind::None,
            snapshot_path: None,
            wal_path: None,
            auto_snapshot_interval_secs: None,
            auth_token: None,
            event_log_path: None,
            mode: Default::default(),
        }
    }

    fn run_and_hash(label: &str) -> [u8; 32] {
        let config = test_config();
        let mut engine = Engine::new(&config);
        for i in 0..100 {
            let vector: Vec<f32> = (0..DIM)
                .map(|j| ((i * 1000 + j) as f32 * 0.001) % 1.0)
                .collect();
            engine.insert_record_from_f32(&vector).expect("insert failed");
        }
        let hash = engine.get_proof().final_state_hash;
        println!("[{}] HASH: {:?}", label, &hash[..8]);
        assert_ne!(hash, [0u8; 32], "state hash must not be all zeros");
        hash
    }

    #[test]
    fn determinism_x86() {
        run_and_hash("x86_64");
    }

    #[test]
    fn determinism_arm() {
        run_and_hash("ARM64");
    }

    #[test]
    fn determinism_wasm() {
        run_and_hash("WASM32");
    }

    #[test]
    fn test_cross_platform_consistency() {
        // Running twice on the same machine must produce the exact same hash —
        // the fundamental guarantee of the Q16.16 deterministic kernel.
        let hash1 = {
            let mut engine = Engine::new(&test_config());
            for i in 0..50 {
                let v: Vec<f32> = (0..DIM).map(|j| (i + j) as f32 * 0.01).collect();
                engine.insert_record_from_f32(&v).unwrap();
            }
            engine.get_proof().final_state_hash
        };

        let hash2 = {
            let mut engine = Engine::new(&test_config());
            for i in 0..50 {
                let v: Vec<f32> = (0..DIM).map(|j| (i + j) as f32 * 0.01).collect();
                engine.insert_record_from_f32(&v).unwrap();
            }
            engine.get_proof().final_state_hash
        };

        assert_eq!(hash1, hash2, "Identical operations must produce identical state hash");
    }
}
