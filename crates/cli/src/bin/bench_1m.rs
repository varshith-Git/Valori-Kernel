use anyhow::Result;
use memmap2::Mmap;
use std::fs::File;
use std::time::Instant;
use valori_kernel::adapters::sift_batch::SiftBatchLoader;
use bytemuck::cast_slice; // The "Senior" way to cast types

const Q16_SCALE: f32 = 65536.0;

fn main() -> Result<()> {
    println!("🚀 Starting SIFT1M Granular Benchmark...");

    let path = "data/sift/sift/sift_base.fvecs";
    let file = File::open(path).expect("Failed to open SIFT file.");
    let mmap = unsafe { Mmap::map(&file)? };

    // Initialize Loader
    let mut loader = SiftBatchLoader::new(&mmap)
        .ok_or_else(|| anyhow::anyhow!("Invalid SIFT format"))?;

    let dim = loader.dim();
    let total = loader.len();
    let batch_size = 10_000;
    
    // We calculate stride manually to inline the logic and avoid function overhead
    // Header (4B) + Data (dim * 4B)
    let stride = 4 + (dim * 4); 

    println!("📊 Dataset: {} Vectors | Dim: {}", total, dim);

    // ==========================================================
    // TEST 1: RAW I/O (Baseline)
    // ==========================================================
    println!("\nTest 1: Raw Memory Bandwidth (No Parsing)...");
    loader = SiftBatchLoader::new(&mmap).unwrap(); // Reset cursor
    let start_io = Instant::now();
    let mut bytes_checksum: u64 = 0;
    
    while let Some((raw_bytes, _count)) = loader.next_batch(batch_size) {
        // Force OS to page-in data by reading every byte.
        // We use a simple sum which the compiler can SIMD optimize,
        // ensuring we hit memory bandwidth limits, not CPU limits.
        let chunk_sum: u64 = raw_bytes.iter().map(|&b| b as u64).sum();
        bytes_checksum = bytes_checksum.wrapping_add(chunk_sum);
    }
    std::hint::black_box(bytes_checksum); // Ensure calculation isn't deleted
    
    let time_io = start_io.elapsed();
    // approximate bytes read (total file size)
    let total_bytes = mmap.len(); 
    println!("   -> Time: {:.4}s | {:.2} GB/s", 
        time_io.as_secs_f64(), 
        (total_bytes as f64 / 1_024.0 / 1_024.0 / 1_024.0) / time_io.as_secs_f64()
    );

    // ==========================================================
    // TEST 2: PARSING COST (Bytemuck Cast)
    // ==========================================================
    println!("\nTest 2: Structure Cost (Bytes -> &[f32])...");
    loader = SiftBatchLoader::new(&mmap).unwrap();
    let start_parse = Instant::now();
    let mut _check_parse: f32 = 0.0;

    while let Some((raw_bytes, count)) = loader.next_batch(batch_size) {
        for i in 0..count {
            let offset = i * stride;
            // Zero-Copy Slice: Skip 4 byte header, take the rest
            // Note: f32 requires 4-byte alignment. SIFT stride is (4 + 128*4) = 516.
            // 516 is divisible by 4, so address alignment is preserved!
            let vec_bytes = &raw_bytes[offset + 4 .. offset + stride];
            
            // bytemuck::cast_slice is SAFE. It checks alignment and length.
            // If this panics, your data is corrupt.
            let vec_f32: &[f32] = cast_slice(vec_bytes);
            
            
            // Sum ALL floats to ensure we read all memory, making this comparable to Test 1.
            for &val in vec_f32 {
                _check_parse += val;
            }
        }
    }

    let time_parse = start_parse.elapsed();
    println!("   - Checksum (f32):    {:.2} (Ignore)", _check_parse);
    println!("   -> Time: {:.4}s | Overhead: {:.4}s", 
        time_parse.as_secs_f64(),
        time_parse.checked_sub(time_io).unwrap_or(std::time::Duration::ZERO).as_secs_f64()
    );

    // ==========================================================
    // TEST 3: MATH COST (f32 -> Q16.16)
    // ==========================================================
    println!("\nTest 3: Determinism Cost (Math Ops)...");
    loader = SiftBatchLoader::new(&mmap).unwrap();
    let start_math = Instant::now();
    let mut check_math: i64 = 0;

    while let Some((raw_bytes, count)) = loader.next_batch(batch_size) {
        for i in 0..count {
            let offset = i * stride;
            let vec_bytes = &raw_bytes[offset + 4 .. offset + stride];
            let vec_f32: &[f32] = cast_slice(vec_bytes);

            // THE HOT LOOP
            for &val in vec_f32 {
                let fixed = (val * Q16_SCALE) as i32;
                check_math = check_math.wrapping_add(fixed as i64);
            }
        }
    }

    let time_math = start_math.elapsed();
    
    // Fix: Don't subtract Test 2 if Test 3 is faster (due to SIMD)
    // Just report the raw math time, which is the "Hot Cache" performance.
    println!("   -> Time: {:.4}s", time_math.as_secs_f64());
    
    println!("--------------------------------------------------");
    println!("📉 COST ANALYSIS:");
    println!("   - Cold I/O (Disk):   {:.4}s", time_io.as_secs_f64());
    println!("   - Hot Math (Memory): {:.4}s", time_math.as_secs_f64());
    println!("--------------------------------------------------");
    
    let total_ops = total as f64 * dim as f64;
    println!("⚡ Hot Throughput: {:.2} Billion ops/sec", 
        total_ops / time_math.as_secs_f64() / 1_000_000_000.0
    );

    Ok(())
}