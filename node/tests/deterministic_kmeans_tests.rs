use valori_node::structure::deterministic::kmeans::deterministic_kmeans;

#[test]
fn test_kmeans_bit_identical() {
    // Generate some deterministic data
    let mut records = Vec::new();
    for i in 0..100 {
        let val = (i as f32) / 100.0;
        records.push((i as u32, vec![val, 1.0 - val]));
    }
    
    // Sort logic should be handled by algo, but we pass sorted to be safe as per doc
    // (Our implementation handles it or expects it? implementation just takes slice)
    
    // Run 1
    let centroids1 = deterministic_kmeans(&records, 5, 10);
    
    // Run 2
    let centroids2 = deterministic_kmeans(&records, 5, 10);
    
    assert_eq!(centroids1.len(), 5);
    
    // Check bit-exact equality
    for (i, c1) in centroids1.iter().enumerate() {
        let c2 = &centroids2[i];
        assert_eq!(c1.len(), c2.len());
        for (j, v1) in c1.iter().enumerate() {
            let v2 = c2[j];
            assert_eq!(v1.to_bits(), v2.to_bits(), "Mismatch at centroid {} dim {}", i, j);
        }
    }
}
