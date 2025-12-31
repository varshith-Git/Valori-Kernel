use anyhow::Result;
use std::time::Instant;
use valori_kernel::ValoriKernel;
use valori_kernel::hnsw::ValoriHNSW;
use valori_kernel::adapters::sift_batch::SiftBatchLoader;
use memmap2::Mmap;
use std::fs::File;
use bytemuck::cast_slice;

const Q16_SCALE: f32 = 65536.0;
const DIM: usize = 128;

fn main() -> Result<()> {
    println!("🚀 Starting Persistence Benchmark...");

    let db_path = "valori_bench.db";
    let _ = std::fs::remove_file(db_path);

    // 1. INGEST (Build the Graph)
    let ingest_limit = 50_000; // Use reasonable size to measure save/load
    println!("📥 Ingesting {} vectors...", ingest_limit);
    
    let mut kernel = ValoriKernel::new();
    let path = "data/sift/sift/sift_base.fvecs";
    let file = File::open(path).expect("Failed to open SIFT base file");
    let mmap = unsafe { Mmap::map(&file)? };
    let mut loader = SiftBatchLoader::new(&mmap).unwrap();

    let mut id_counter = 0;
    // Buffer: CMD(1) + ID(8) + DIM(2) + VEC(DIM*4) + TAG(8)
    let mut packet_buffer = vec![0u8; 1 + 8 + 2 + (DIM * 4) + 8]; 
    packet_buffer[0] = 1; // CMD_INSERT
    packet_buffer[9..11].copy_from_slice(&(DIM as u16).to_le_bytes());

    while let Some((raw_bytes, count)) = loader.next_batch(1000) {
        let stride = 4 + (DIM * 4);
        for i in 0..count {
            if id_counter >= ingest_limit { break; }
            let offset = i * stride;
            let vec_f32: &[f32] = cast_slice(&raw_bytes[offset+4 .. offset+stride]);
            
            // Construct Payload
            let id = id_counter as u64;
            packet_buffer[1..9].copy_from_slice(&id.to_le_bytes());
            
            // Vector
            let payload_vec_start = 11;
            let payload_vec_end = 11 + (DIM * 4);
            let payload_vec = &mut packet_buffer[payload_vec_start..payload_vec_end];
            for (j, &val) in vec_f32.iter().enumerate() {
                let fixed = (val * Q16_SCALE) as i32;
                payload_vec[j*4..(j+1)*4].copy_from_slice(&fixed.to_le_bytes());
            }
            
            // Tag (Default 0)
            packet_buffer[payload_vec_end..payload_vec_end+8].copy_from_slice(&0u64.to_le_bytes());

            kernel.apply_event(&packet_buffer)?;
            id_counter += 1;
        }
        if id_counter >= ingest_limit { break; }
    }
    println!("✅ Ingest Complete. Graph Size: {}", kernel.record_count());

    // 2. SAVE SNAPSHOT
    println!("💾 Saving Snapshot to {}...", db_path);
    let start_save = Instant::now();
    // We need to access the inner index to call save, or expose save on kernel?
    // Using `pub index` from kernel structure (it is pub in ValoriKernel?)
    // `kernel.index` is public? Let's assume so or fix it.
    kernel.index.save(db_path)?;
    let save_time = start_save.elapsed();
    println!("✅ Save Complete in {:.2?}", save_time);

    // 3. DROP KERNEL
    drop(kernel);
    println!("🗑️  Kernel Dropped from RAM.");

    // 4. LOAD SNAPSHOT
    println!("📂 Loading Snapshot from {}...", db_path);
    let start_load = Instant::now();
    let loaded_index = ValoriHNSW::load(db_path)?;
    let load_time = start_load.elapsed();
    println!("✅ Load Complete in {:.2?}", load_time);
    
    // Verify restored state
    let restored_kernel = ValoriKernel { index: loaded_index };
    println!("📊 Restored Graph Size: {}", restored_kernel.record_count());
    assert_eq!(restored_kernel.record_count(), ingest_limit);

    // 5. QUERY CHECK
    println!("🔎 Verifying Query...");
    let q_vec = vec![0i32; DIM]; // Zero query just to see if it runs
    let results = restored_kernel.search(&q_vec, 1, None)?;
    println!("Got {} results. First: {:?}", results.len(), results.get(0));

    println!("--------------------------------------------------");
    println!("⏱️  PERSISTENCE REPORT ({} Vectors)", ingest_limit);
    println!("   - Save Time: {:.2?}", save_time);
    println!("   - Load Time: {:.2?}", load_time);
    println!("--------------------------------------------------");

    let _ = std::fs::remove_file(db_path);
    Ok(())
}
