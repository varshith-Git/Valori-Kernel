use anyhow::Result;
use std::time::Instant;
use valori_kernel::ValoriKernel;
use valori_kernel::adapters::sift_batch::SiftBatchLoader;
use memmap2::Mmap;
use std::fs::File;
use bytemuck::cast_slice;

const Q16_SCALE: f32 = 65536.0;
const DIM: usize = 128;

fn main() -> Result<()> {
    println!("🚀 Starting SIFT1M Recall Benchmark...");

    // 1. INGEST (Build the Graph)
    let ingest_limit = 10_000; 
    
    println!("📥 Ingesting {} vectors into Arena...", ingest_limit);
    let mut kernel = ValoriKernel::new();
    let path = "data/sift/sift/sift_base.fvecs"; // Path adjusted to match bench_ingest
    let file = File::open(path).expect("Failed to open SIFT base file");
    let mmap = unsafe { Mmap::map(&file)? };
    let mut loader = SiftBatchLoader::new(&mmap).unwrap();

    let mut id_counter = 0;
    // Pre-allocate buffer: CMD(1) + ID(8) + DIM(2) + VEC(128*4)
    // Buffer: CMD(1) + ID(8) + DIM(2) + VEC(DIM*4) + TAG(8)
    let buffer_size = 1 + 8 + 2 + (DIM * 4) + 8;
    let mut packet_buffer = vec![0u8; buffer_size]; 
    packet_buffer[0] = 1; // CMD_INSERT
    packet_buffer[9..11].copy_from_slice(&(DIM as u16).to_le_bytes());

    // This will store the ground truth vectors for brute-force comparison
    let mut ground_truth_vectors = Vec::with_capacity(ingest_limit);

    while let Some((raw_bytes, count)) = loader.next_batch(1000) {
        let stride = 4 + (DIM * 4);
        for i in 0..count {
            if id_counter >= ingest_limit { break; }
            
            let offset = i * stride;
            let vec_f32: &[f32] = cast_slice(&raw_bytes[offset+4 .. offset+stride]);
            
            // Ground Truth Store
            let mut gt_vec = Vec::with_capacity(DIM);
            for &val in vec_f32 {
                gt_vec.push((val * Q16_SCALE) as i32);
            }
            ground_truth_vectors.push((id_counter as u64, gt_vec));

            // Construct Payload for Kernel
            let id = id_counter as u64;
            packet_buffer[1..9].copy_from_slice(&id.to_le_bytes());
            
            let vec_start = 11;
            let vec_end = 11 + (DIM * 4);
            let payload_vec = &mut packet_buffer[vec_start..vec_end];
            for (j, &val) in vec_f32.iter().enumerate() {
                let fixed = (val * Q16_SCALE) as i32;
                payload_vec[j*4..(j+1)*4].copy_from_slice(&fixed.to_le_bytes());
            }
            
            // Tag (Default 0)
            packet_buffer[vec_end..vec_end+8].copy_from_slice(&0u64.to_le_bytes());

            kernel.apply_event(&packet_buffer)?;
            id_counter += 1;
        }
        if id_counter >= ingest_limit { break; }
    }
    println!("✅ Ingest Complete. Graph Size: {}", kernel.record_count()); // changed count() to record_count()

    // 2. QUERY (Measure Accuracy vs Brute Force on Subset)
    println!("🔎 Running Queries against Ground Truth (Brute Force Check on Subset)...");
    
    // Load Queries
    let q_path = "data/sift/sift/sift_query.fvecs";
    let q_file = File::open(q_path).expect("Failed to open SIFT query file");
    let q_mmap = unsafe { Mmap::map(&q_file)? };
    let mut q_loader = SiftBatchLoader::new(&q_mmap).unwrap();

    let mut hits_at_1 = 0;
    let mut hits_at_10 = 0;
    let mut total_queries = 0;
    let start_search = Instant::now();
    
    // We need access to the data to run brute force.
    // The kernel stores it. We can't access kernel internals easily from CLI binary without hacks 
    // or adding a "get_vector" API.
    // But `bench_recall` ingests data. We can keep a copy?
    // Memory usage: 10k * 128 * 4 bytes = 5MB. Trivial.
    // Let's reload the 10k vectors into a simple Vec for BF.
    
    let mut reference_vectors = Vec::with_capacity(ingest_limit);
    {
         let path = "data/sift/sift/sift_base.fvecs";
         let file = File::open(path)?;
         let mmap = unsafe { Mmap::map(&file)? };
         let mut loader = SiftBatchLoader::new(&mmap).unwrap();
         let mut cnt = 0;
         while let Some((raw, n)) = loader.next_batch(1000) {
             let stride = 4 + (DIM * 4);
             for i in 0..n {
                 if cnt >= ingest_limit { break; }
                 let offset = i * stride;
                 let vec_f32: &[f32] = cast_slice(&raw[offset+4 .. offset+stride]);
                 let mut fix = vec![0i32; DIM];
                 for (j, &val) in vec_f32.iter().enumerate() {
                     fix[j] = (val * Q16_SCALE) as i32;
                 }
                 reference_vectors.push((cnt as u64, fix));
                 cnt += 1;
             }
         }
    }

    // Processing Loop
    while let Some((q_raw, _count)) = q_loader.next_batch(1) {
        let stride = 4 + (DIM * 4);
        let q_vec_f32: &[f32] = cast_slice(&q_raw[4..stride]);

        let mut q_fixed = vec![0i32; DIM];
        for (j, &val) in q_vec_f32.iter().enumerate() {
            q_fixed[j] = (val * Q16_SCALE) as i32;
        }

        // A. Run Kernel Search
        let results = kernel.search(&q_fixed, 10, None)?; 
        
        // B. Run Brute Force (The Truth)
        let mut best_dist = i64::MAX;
        let mut best_id = 0u64;
        
        for (id, ref_vec) in &reference_vectors {
            // Euclidean Dist Sq
            let mut d: i64 = 0;
            // Unroll slightly to match general speed
            for i in 0..DIM {
                let diff = q_fixed[i] as i64 - ref_vec[i] as i64;
                d += diff * diff;
            }
            if d < best_dist {
                best_dist = d;
                best_id = *id;
            }
        }

        if results.is_empty() { 
             total_queries += 1;
             continue; 
        }

        let top_1_id = results[0].0; 

        // Check Recall @ 1
        if top_1_id == best_id {
            hits_at_1 += 1;
        } else {
             // Debug print only if totally wrong distance
             // Sometimes distances are identical for different IDs.
             if results[0].1 == best_dist {
                 hits_at_1 += 1; // Count as hit if distance matches
             }
        }

        // Check Recall @ 10
        for (res_id, res_dist) in &results {
            if *res_id == best_id || *res_dist == best_dist {
                hits_at_10 += 1;
                break;
            }
        }

        total_queries += 1;
        if total_queries >= 100 { break; } 
    }

    let search_time = start_search.elapsed();
    
    println!("--------------------------------------------------");
    println!("🎯 RECALL REPORT ({} Queries) vs {} Vector Subset", total_queries, ingest_limit);
    println!("   - Recall@1:  {:.2}%", hits_at_1 as f64 / total_queries as f64 * 100.0);
    println!("   - Recall@10: {:.2}%", hits_at_10 as f64 / total_queries as f64 * 100.0);
    println!("   - Latency:   {:.2} ms/query", (search_time.as_millis() as f64 / total_queries as f64));
    println!("--------------------------------------------------");
    
    // Pass/Fail Logic
    if total_queries > 0 {
        let r1 = (hits_at_1 as f64 / total_queries as f64);
        if r1 > 0.90 {
            println!("🚀 SUCCESS: Recall@1 > 90%");
        } else {
             println!("⚠️  WARNING: Recall@1 < 90%. Tuning needed.");
        }
    }

    Ok(())
}
