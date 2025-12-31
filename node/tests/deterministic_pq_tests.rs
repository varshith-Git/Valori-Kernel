// Copyright (c) 2025 Varshith Gudur. Licensed under AGPLv3.
use valori_node::structure::quant::Quantizer;
use valori_node::structure::quant::pq::{ProductQuantizer, PqConfig};

#[test]
fn test_pq_roundtrip() {
    let dim = 16;
    let mut records = Vec::new();
    for i in 0..100 {
        records.push((i as u32, vec![0.5; dim]));
    }
    
    // Config: 4 subvectors (4 dims each), 16 centroids
    let cfg = PqConfig { n_subvectors: 4, n_centroids: 16 };
    let mut pq = ProductQuantizer::new(cfg, dim);
    
    // Build
    pq.build(&records);
    
    // Snapshot
    let snap = pq.snapshot().unwrap();
    
    // Restore
    let mut pq2 = ProductQuantizer::new(PqConfig::default(), 0); // Dummy
    pq2.restore(&snap).unwrap();
    
    // Test Encode/Decode
    let vec = vec![0.5; dim];
    let codes = pq.quantize(&vec);
    assert_eq!(codes.len(), 4);
    
    let codes2 = pq2.quantize(&vec);
    assert_eq!(codes, codes2);
    
    let rec = pq2.reconstruct(&codes);
    assert_eq!(rec.len(), dim);
    
    // Should be close to [0.5...] (since 0.5 was in training set)
    let dist: f32 = vec.iter().zip(rec).map(|(a, b)| (a-b).powi(2)).sum();
    assert!(dist < 0.1, "Reconstruction error too high: {}", dist);
}
