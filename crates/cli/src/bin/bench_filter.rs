use anyhow::Result;
use valori_kernel::{ValoriKernel, types::FixedPointVector};

fn main() -> Result<()> {
    println!("🚀 Starting Metadata Filter Benchmark...");
    let mut kernel = ValoriKernel::new();
    
    let dim = 128;

    // 1. Ingest Data with Tags
    // Even IDs -> Tag 1 (Red)
    // Odd IDs  -> Tag 2 (Blue)
    println!("📥 Ingesting 10,000 tagged vectors...");
    for i in 0..10_000u64 {
        let vec = vec![0; dim]; // Dummy vector
        let tag = if i % 2 == 0 { 1 } else { 2 };
        
        // Use the new insert helper
        kernel.insert(i, vec, tag)?; 
    }

    // 2. Search with Filter (Tag 1)
    println!("🔎 Searching for Tag 1 (Evens)...");
    let query = vec![0; dim];
    // None = No filter, Some(1) = Filter for Tag 1
    let results = kernel.search(&query, 10, Some(1))?;

    // 3. Verify
    println!("📊 Got {} results", results.len());
    for (id, _) in results {
        if id % 2 != 0 {
            panic!("❌ FAILED: Found Odd ID {} inside Tag 1 results!", id);
        }
    }
    
    println!("✅ SUCCESS: Filter strictly enforced. All results have Tag 1.");

    // 4. Search with Filter (Tag 2)
    println!("🔎 Searching for Tag 2 (Odds)...");
    let results2 = kernel.search(&query, 10, Some(2))?;
    for (id, _) in results2 {
        if id % 2 == 0 {
            panic!("❌ FAILED: Found Even ID {} inside Tag 2 results!", id);
        }
    }
    println!("✅ SUCCESS: Filter strictly enforced for Tag 2.");

    println!("✅ Architecture supports Hybrid Search (Vector + Metadata).");
    Ok(())
}
