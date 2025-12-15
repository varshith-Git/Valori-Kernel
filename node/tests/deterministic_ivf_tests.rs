// Copyright (c) 2025 Varshith Gudur. Licensed under AGPLv3.
use valori_kernel::structure::ivf::{IvfIndex, IvfConfig};
use valori_node::structure::index::VectorIndex;

#[test]
fn test_ivf_determinism() {
    let mut records = Vec::new();
    for i in 0..200 {
        let val = (i as f32) / 200.0;
        records.push((i as u32, vec![val, val, 1.0 - val]));
    }
    
    // Run 1
    let mut ivf1 = IvfIndex::new(IvfConfig { n_list: 10, n_probe: 3 }, 3);
    ivf1.build(&records);
    let snap1 = ivf1.snapshot().unwrap();
    
    // Run 2
    let mut ivf2 = IvfIndex::new(IvfConfig { n_list: 10, n_probe: 3 }, 3);
    ivf2.build(&records);
    let snap2 = ivf2.snapshot().unwrap();
    
    assert_eq!(snap1, snap2, "Index snapshots must be identical");
    
    // Test Search Consistency
    let query = vec![0.5, 0.5, 0.5];
    let res1 = ivf1.search(&query, 5);
    let res2 = ivf2.search(&query, 5);
    
    assert_eq!(res1, res2);
}

#[test]
fn test_ivf_restore() {
      let mut records = Vec::new();
    for i in 0..100 {
        records.push((i as u32, vec![1.0; 3]));
    }
    
    let mut ivf1 = IvfIndex::new(IvfConfig::default(), 3);
    ivf1.build(&records);
    let snap = ivf1.snapshot().unwrap();
    
    let mut ivf2 = IvfIndex::new(IvfConfig::default(), 3);
    ivf2.restore(&snap).unwrap();
    
    // Check if internal state matches (via search)
    let res1 = ivf1.search(&[1.0, 1.0, 1.0], 5);
    let res2 = ivf2.search(&[1.0, 1.0, 1.0], 5);
    assert_eq!(res1, res2);
}
