// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
use valori_node::structure::deterministic::kmeans::deterministic_kmeans;

/// Q16.16 scale factor — same value used in the kernel.
const SCALE: f32 = 65536.0;

fn to_q16(f: f32) -> i32 {
    (f * SCALE).round() as i32
}

#[test]
fn test_kmeans_bit_identical() {
    let mut records = Vec::new();
    for i in 0..100 {
        let val = (i as f32) / 100.0;
        records.push((i as u32, vec![val, 1.0 - val]));
    }

    let centroids1 = deterministic_kmeans(&records, 5, 10);
    let centroids2 = deterministic_kmeans(&records, 5, 10);

    assert_eq!(centroids1.len(), 5);
    assert_eq!(
        centroids1, centroids2,
        "Same inputs must produce bit-identical Q16.16 centroids"
    );
}
