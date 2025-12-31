use anyhow::Result;
use memmap2::Mmap;
use std::fs::File;
use std::time::Instant;
use valori_kernel::ValoriKernel; // The Engine
use valori_kernel::adapters::sift_batch::SiftBatchLoader;
use bytemuck::cast_slice;

// Standard SIFT1M is 128 dims
const DIM: usize = 128;
const Q16_SCALE: f32 = 65536.0;

fn main() -> Result<()> {
    println!("🚀 Starting Kernel Ingestion Benchmark (End-to-End)...");

    // 1. Setup Data
    let path = "data/sift/sift/sift_base.fvecs";
    let file = File::open(path).expect("Failed to open SIFT file");
    let mmap = unsafe { Mmap::map(&file)? };
    let mut loader = SiftBatchLoader::new(&mmap)
        .ok_or_else(|| anyhow::anyhow!("Invalid SIFT format"))?;

    println!("📊 Dataset: {} Vectors", loader.len());

    // 2. Initialize Kernel
    // This is the "System Under Test"
    let mut kernel = ValoriKernel::new();
    println!("🤖 Kernel Initialized. Ready for Ingest.");

    // 3. Setup Reusable Buffer (Zero-Alloc Loop)
    // Payload: [CMD(1)] + [ID(8)] + [DIM(2)] + [VALUES(128*4)]
    // Based on `crates/kernel/src/types.rs`
    let packet_size = 1 + 8 + 2 + (DIM * 4);
    let mut packet_buffer = vec![0u8; packet_size];
    
    // Constant Header Fields
    // Offset 0: CMD_INSERT = 1
    // Buffer: CMD(1) + ID(8) + DIM(2) + VEC(DIM*4) + TAG(8)
    let buffer_size = 1 + 8 + 2 + (DIM * 4) + 8;
    let mut packet_buffer = vec![0u8; buffer_size]; 
    packet_buffer[0] = 1; // CMD_INSERT
    packet_buffer[9..11].copy_from_slice(&(DIM as u16).to_le_bytes());

    let ingest_limit = loader.len(); // Ingest all available vectors
    let mut id_counter = 0;
    
    println!("🏁 Ingestion Started...");
    let start = Instant::now();

    while let Some((raw_bytes, count)) = loader.next_batch(1000) {
        let stride = 4 + (DIM * 4);
        for i in 0..count {
            if id_counter >= ingest_limit { break; }
            let offset = i * stride; // fvecs format
            let vec_f32: &[f32] = cast_slice(&raw_bytes[offset+4 .. offset+stride]);
            
            // Construct Payload
            // ID
            let id = id_counter as u64;
            packet_buffer[1..9].copy_from_slice(&id.to_le_bytes());
            
            // Vector
            let vec_start = 11;
            let vec_end = 11 + (DIM * 4);
            let payload_vec = &mut packet_buffer[vec_start..vec_end];
            for (j, &val) in vec_f32.iter().enumerate() {
                let fixed = (val * Q16_SCALE) as i32;
                payload_vec[j*4..(j+1)*4].copy_from_slice(&fixed.to_le_bytes());
            }

            // Tag (Default 0)
            packet_buffer[vec_end..vec_end+8].copy_from_slice(&0u64.to_le_bytes());

            // The Critical Call (Apply to State)
            // This tests the Kernel's locking, hashing, and storage logic.
            kernel.apply_event(&packet_buffer)?;
            id_counter += 1;

            if id_counter % 5000 == 0 {
                // simple println to avoid cursor jump issues in automation
                println!("Ingesting: {} ...", id_counter);
            }
        }
    }
    println!();

    let duration = start.elapsed();
    let seconds = duration.as_secs_f64();
    let eps = id_counter as f64 / seconds;

    println!("--------------------------------------------------");
    println!("✅ INGESTION COMPLETE.");
    println!("   - Events:     {}", id_counter);
    println!("   - Time:       {:.4} seconds", seconds);
    println!("   - Throughput: {:.2} EPS (Events Per Second)", eps);
    println!("--------------------------------------------------");
    
    // Check if we hit the target
    if eps > 10_000.0 {
        println!("🚀 SUCCESS: Speed > 10k EPS");
    } else {
        println!("⚠️  WARNING: Speed < 10k EPS. Optimization needed.");
    }

    Ok(())
}