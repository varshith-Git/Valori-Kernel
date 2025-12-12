use valori_node::structure::deterministic::kmeans::deterministic_kmeans;
use valori_node::structure::ivf::{IvfIndex, IvfConfig};
use valori_node::structure::index::VectorIndex;
use valori_node::engine::Engine;
use valori_node::config::{NodeConfig, IndexKind, QuantizationKind};
use std::sync::Arc;

#[test]
fn test_kmeans_empty() {
    let empty_records: Vec<(u32, Vec<f32>)> = Vec::new();
    let centroids = deterministic_kmeans(&empty_records, 5, 10);
    assert!(centroids.is_empty(), "Should return empty centroids for empty input");
}

#[test]
fn test_kmeans_k_greater_than_n() {
    let mut records = Vec::new();
    for i in 0..3 {
        records.push((i as u32, vec![1.0, 2.0]));
    }
    // Request k=5 but only 3 records
    let centroids = deterministic_kmeans(&records, 5, 10);
    assert_eq!(centroids.len(), 3, "Should return N centroids if K >= N");
    // Should be sorted by ID (which effectively means input order if inserted sorted)
    // Centroid 0 should matches record 0
    assert_eq!(centroids[0], vec![1.0, 2.0]);
}

#[test]
#[should_panic(expected = "All vectors must share the same dimension")]
fn test_kmeans_dimension_mismatch() {
    let records = vec![
        (0, vec![1.0, 2.0]),
        (1, vec![1.0]), // Dim mismatch
    ];
    deterministic_kmeans(&records, 2, 10);
}

#[test]
fn test_kmeans_rounding_and_rounding() {
    // Test that 32767.99 gets clamped/rounded properly without overflowing i32
    // MAX_SAFE is ~32767.9
    let mut records = Vec::new();
    let val = 30000.0; // Large but safe
    records.push((0, vec![val]));
    records.push((1, vec![val]));
    
    // This should not panic
    let centroids = deterministic_kmeans(&records, 1, 5);
    // Centroid should equal input due to averaging same values
    assert!((centroids[0][0] - val).abs() < 0.01);
}

#[test]
fn test_engine_insert_out_of_range() {
    let mut cfg = NodeConfig::default();
    // Match const generics <10, 1, 10, 10>
    cfg.max_records = 10;
    cfg.dim = 1;
    cfg.max_nodes = 10;
    cfg.max_edges = 10;
    
    let mut engine = Engine::<10, 1, 10, 10>::new(&cfg);
    
    // > 32767.99
    let bad_val = vec![33000.0]; 
    let res = engine.insert_record_from_f32(&bad_val);
    assert!(res.is_err());
    
    // < -32768.0
    let bad_val_neg = vec![-33000.0];
    let res_neg = engine.insert_record_from_f32(&bad_val_neg);
    assert!(res_neg.is_err());
    
    // Valid
    let good = vec![32000.0];
    assert!(engine.insert_record_from_f32(&good).is_ok());
}

#[test]
fn test_pq_overflow_handling() {
    use valori_node::structure::quant::pq::{ProductQuantizer, PqConfig};
    
    let dim = 4;
    let mut records = Vec::new();
    // Extremes that fit in scaling
    records.push((0, vec![32000.0; dim]));
    records.push((1, vec![-32000.0; dim]));
    
    // Config: 1 subvector
    let cfg = PqConfig { n_subvectors: 1, n_centroids: 2 };
    let mut pq = ProductQuantizer::new(cfg, dim);
    
    // Should handle large values gracefully (rounding/clamping in kmeans)
    pq.build(&records);
    
    let snap = pq.snapshot().unwrap();
    assert!(snap.len() > 0);
}
