// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
use valori_node::structure::deterministic::kmeans::deterministic_kmeans;
use valori_node::structure::ivf::{IvfIndex, IvfConfig};
use valori_node::structure::index::VectorIndex;
use valori_node::engine::Engine;
use valori_node::config::{NodeConfig, IndexKind, QuantizationKind};

/// Q16.16 conversion — matches what the kernel does internally.
fn to_q16(f: f32) -> i32 {
    (f * 65536.0).round() as i32
}

#[test]
fn test_kmeans_empty() {
    let empty: Vec<(u32, Vec<f32>)> = Vec::new();
    let centroids = deterministic_kmeans(&empty, 5, 10);
    assert!(centroids.is_empty(), "Should return empty centroids for empty input");
}

#[test]
fn test_kmeans_k_greater_than_n() {
    let records = vec![
        (0u32, vec![1.0f32, 2.0]),
        (1u32, vec![1.0, 2.0]),
        (2u32, vec![1.0, 2.0]),
    ];
    // k=5 > n=3 → should return n=3 centroids, each equal to the single unique value.
    let centroids = deterministic_kmeans(&records, 5, 10);
    assert_eq!(centroids.len(), 3, "Should return N centroids when K >= N");
    // All centroids should equal [1.0, 2.0] in Q16.16.
    assert_eq!(centroids[0], vec![to_q16(1.0), to_q16(2.0)]);
}

#[test]
#[should_panic(expected = "All vectors must share the same dimension")]
fn test_kmeans_dimension_mismatch() {
    let records = vec![
        (0, vec![1.0, 2.0]),
        (1, vec![1.0]),
    ];
    deterministic_kmeans(&records, 2, 10);
}

#[test]
fn test_kmeans_rounding_does_not_overflow() {
    // Large-but-safe Q16.16 values should not overflow i32 during centroid computation.
    let records = vec![
        (0u32, vec![30000.0f32]),
        (1u32, vec![30000.0]),
    ];
    let centroids = deterministic_kmeans(&records, 1, 5);
    assert!(!centroids.is_empty());
    // Centroid of identical values must equal that value.
    let expected = to_q16(30000.0);
    assert!((centroids[0][0] - expected).abs() <= 1, "centroid should round-trip 30000.0");
}

#[test]
fn test_engine_insert_out_of_range() {
    let mut cfg = NodeConfig::default();
    cfg.dim = 1;
    cfg.max_records = 10;
    cfg.max_nodes = 10;
    cfg.max_edges = 10;

    let mut engine = Engine::new(&cfg);

    // Values outside the Q16.16 safe range must be rejected.
    assert!(engine.insert_record_from_f32(&[33000.0]).is_err(), ">32767.99 must be rejected");
    assert!(engine.insert_record_from_f32(&[-33000.0]).is_err(), "<-32768 must be rejected");

    // Value within range must succeed.
    assert!(engine.insert_record_from_f32(&[32000.0]).is_ok());
}

#[test]
fn test_pq_overflow_handling() {
    use valori_node::structure::quant::pq::{ProductQuantizer, PqConfig};

    let dim = 4;
    let records = vec![
        (0u32, vec![32000.0f32; dim]),
        (1u32, vec![-32000.0; dim]),
    ];
    let cfg = PqConfig { n_subvectors: 1, n_centroids: 2 };
    let mut pq = ProductQuantizer::new(cfg, dim);
    pq.build(&records);

    let snap = pq.snapshot().unwrap();
    assert!(!snap.is_empty());
}
