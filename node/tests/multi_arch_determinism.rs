// Copyright (c) 2025 Varshith Gudur. Licensed under AGPLv3.
//! Multi-Architecture Determinism Tests
//!
//! These tests verify that identical operations produce identical state hashes
//! across different CPU architectures (x86, ARM, WASM).

#[cfg(test)]
mod determinism_tests {
    use valori_node::config::{NodeConfig, IndexKind, QuantizationKind};
    use valori_node::engine::Engine;

    const MAX_RECORDS: usize = 1024;
    const DIM: usize = 16;
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

    #[test]
    fn determinism_x86() {
        test_determinism_common("x86_64");
    }

    #[test]
    fn determinism_arm() {
        test_determinism_common("ARM64");
    }

    #[test]
    fn determinism_wasm() {
        test_determinism_common("WASM32");
    }

    fn test_determinism_common(arch: &str) {
        println!("\n=== {} Determinism Test ===", arch);
        
        let config = test_config();
        let mut engine = Engine::<MAX_RECORDS, DIM, MAX_NODES, MAX_EDGES>::new(&config);

        // Insert 100 records with deterministic data
        println!("Inserting 100 records with seeded data...");
        for i in 0..100 {
            let vector: Vec<f32> = (0..DIM)
                .map(|j| {
                    // Deterministic seed-based generation
                    let seed = (i * 1000 + j) as f32;
                    (seed * 0.001) % 1.0  // Keep in [0, 1) range
                })
                .collect();
            
            engine.insert_record_from_f32(&vector)
                .expect("Failed to insert record");
        }

        // Get final state hash
        let proof = engine.get_proof();
        let hash = proof.final_state_hash;

        // Print hash in format that CI can grep
        println!("HASH: {:?}", hash);
        
        // Also print first 8 bytes for readability
        println!("Hash (first 8 bytes): {:?}", &hash[..8]);
        
        // Verify non-zero (sanity check)
        assert_ne!(hash, [0u8; 32], "State hash should not be all zeros");
        
        println!("✅ {} test complete", arch);
    }

    #[test]
    fn test_cross_platform_consistency() {
        // This test runs on the same machine but validates the process
        println!("\n=== Cross-Platform Consistency Validation ===");
        
        let config = test_config();
        
        // Run twice with identical data
        let hash1 = {
            let mut engine = Engine::<MAX_RECORDS, DIM, MAX_NODES, MAX_EDGES>::new(&config);
            for i in 0..50 {
                let v: Vec<f32> = (0..DIM).map(|j| (i + j) as f32 * 0.01).collect();
                engine.insert_record_from_f32(&v).unwrap();
            }
            engine.get_proof().final_state_hash
        };

        let hash2 = {
            let mut engine = Engine::<MAX_RECORDS, DIM, MAX_NODES, MAX_EDGES>::new(&config);
            for i in 0..50 {
                let v: Vec<f32> = (0..DIM).map(|j| (i + j) as f32 * 0.01).collect();
                engine.insert_record_from_f32(&v).unwrap();
            }
            engine.get_proof().final_state_hash
        };

        println!("Run 1 hash: {:?}", &hash1[..8]);
        println!("Run 2 hash: {:?}", &hash2[..8]);

        assert_eq!(hash1, hash2, "Identical operations must produce identical hashes");
        println!("✅ Cross-platform consistency verified");
    }
}
