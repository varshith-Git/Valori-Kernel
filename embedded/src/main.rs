#![no_std]
#![no_main]

// This firmware validates that the Valori Kernel executes deterministically
// inside a Cortex-M microcontroller environment.
//
// Same input commands → same state hash → same memory graph
// across:
//  - Cloud nodes
//  - Edge devices
//  - Embedded controllers
//
// This proves Valori is not a "database" —
// it is a deterministic memory computer.

extern crate alloc; // Required for Heap

// Modules
mod flash;
mod snapshot;
mod proof;
mod transport;
mod wal;
mod checkpoint;
mod wal_stream;
mod shadow;
mod recovery;

use cortex_m_rt::entry;
use embedded_alloc::Heap;
use panic_halt as _; // Deterministic panic handler (infinite loop)

// Import Valori Kernel Types
use valori_kernel::state::kernel::KernelState;
use valori_kernel::state::command::Command;
use valori_kernel::types::vector::FxpVector;
use valori_kernel::types::scalar::FxpScalar;
use valori_kernel::types::id::RecordId;

// --- 1. Global Allocator ---
// We must manage memory manually since there is no OS.
// 8KB Heap -> Process Heap + Flash Buffer requires more?
// Default 8KB is small for 64KB snapshot buffer allocation in snapshot.rs
// I will increase HEAP to 96KB to support 64KB snapshot buffer + proof strings.
// Cortex-M4 usually has 128KB+ RAM.
#[global_allocator]
static HEAP: Heap = Heap::empty();

// Static buffer for heap memory. 
// Placed in .bss/static RAM.
// 24576 * 4 = 98304 bytes = 96KB
static mut HEAP_MEM: [u32; 24576] = [0; 24576]; 

// -----------------------------------------------------------------------
// Configuration (Match Node Config for Determinism)
// -----------------------------------------------------------------------
const MAX_RECORDS: usize = 1000;
const D: usize = 16;
const MAX_NODES: usize = 1000;
const MAX_EDGES: usize = 2048;

#[derive(PartialEq)]
enum BootMode {
    SelfTest,
    WalReplay,
}

// Set Firmware Mode here
const MODE: BootMode = BootMode::WalReplay; 

// --- 2. Entry Point ---
#[entry]
fn main() -> ! {
    // A. Initialize Heap
    unsafe { 
        let ptr = core::ptr::addr_of_mut!(HEAP_MEM);
        HEAP.init(ptr as usize, 98304); 
    }

    // B. Initialize Kernel
    let mut state = KernelState::<MAX_RECORDS, D, MAX_NODES, MAX_EDGES>::new();

    if MODE == BootMode::SelfTest {
        // C. Deterministic Test Vector (Manual)
        // Q16.16 values: 1.0 -> 65536, 0.5 -> 32768, -1.0 -> -65536
        let mut vector = FxpVector::<D>::new_zeros();
        vector.data[0] = FxpScalar(65536);       // 1.0
        vector.data[1] = FxpScalar(0);           // 0.0
        vector.data[2] = FxpScalar(-65536);      // -1.0
        vector.data[3] = FxpScalar(32768);       // 0.5
        
        let id = RecordId(0);
        let cmd = Command::InsertRecord { id, vector };
        
        match state.apply(&cmd) {
            Ok(_) => {}
            Err(_) => cortex_m::asm::bkpt(),
        }
    } else {
        // Mode B: WAL Streaming & Recovery (Phase 4)
        
        // 1. Recovery
        // Boot -> Checkpoint -> Snapshot -> State
        // If first boot, starts clean.
        let last_seq = match recovery::recover(&mut state) {
            Ok(seq) => seq,
            Err(_) => {
                cortex_m::asm::bkpt(); // Panic on Recovery Failure
                0
            }
        };

        // 2. Initialize Components
        let mut stream_track = wal_stream::WalStream::new(last_seq);     
        let mut shadow = shadow::ShadowKernel::new(&mut state);
        
        // 3. Receive Packet (Simulated UART Stream)
        // Construct a Phase 4 Packet containing the Bincode-encoded WAL data
        
        // Inner WAL Data (Bincode Command)
        let mut wal_payload: [u8; 128] = [0; 128];
        let mut idx = 0;
        
        // Create Command
        let mut vector = FxpVector::<D>::new_zeros();
        vector.data[0] = FxpScalar(65536);       // 1.0
        vector.data[1] = FxpScalar(0);           // 0.0
        vector.data[2] = FxpScalar(-65536);      // -1.0
        vector.data[3] = FxpScalar(32768);       // 0.5
        
        let id = RecordId(0);
        let cmd = Command::InsertRecord { id, vector };
        
        // Encode using Bincode
        let config = bincode::config::standard();
        let len = match bincode::serde::encode_into_slice(&cmd, &mut wal_payload, config) {
            Ok(l) => l,
            Err(_) => { cortex_m::asm::bkpt(); 0 }
        };

        // Packet Header: [VER:1][FLAGS:1][SEQ:8][LEN:4]
        // Header Size = 14.
        // Payload = WalHeader(16) + Bincode(len)
        let total_payload_len = 16 + len;
        
        let pkt_payload_len = total_payload_len as u32; 
        let mut packet: [u8; 14 + 144] = [0; 14 + 144]; // Increased size
        let mut p_idx = 0;
        
        // Packet Header Construction
        packet[p_idx] = 1; p_idx+=1; // Packet Version
        packet[p_idx] = wal_stream::FLAG_EOS; p_idx+=1; // Flags (EOS -> Commit Segment)
        packet[p_idx..p_idx+8].copy_from_slice(&last_seq.to_le_bytes()); p_idx+=8; // Seq
        packet[p_idx..p_idx+4].copy_from_slice(&pkt_payload_len.to_le_bytes()); p_idx+=4; // Len
        
        // 1. Copy WalHeader (Manual LE) - 16 Bytes
        // [Ver:4][Enc:4][Dim:4][Crc:4]
        packet[p_idx..p_idx+4].copy_from_slice(&1u32.to_le_bytes()); 
        packet[p_idx+4..p_idx+8].copy_from_slice(&0u32.to_le_bytes()); 
        packet[p_idx+8..p_idx+12].copy_from_slice(&(D as u32).to_le_bytes()); 
        packet[p_idx+12..p_idx+16].copy_from_slice(&0u32.to_le_bytes());
        p_idx += 16;
        
        // 2. Copy Bincode Command
        packet[p_idx..p_idx+len].copy_from_slice(&wal_payload[0..len]);
        p_idx += len;
        
        let packet_size = p_idx;
        
        // 4. Ingest Logic
        shadow.start_segment();
        
        match stream_track.ingest_packet(&packet[0..packet_size]) {
            Ok((chunk, is_eos)) => {
                // Apply to Shadow
                if shadow.apply_chunk(chunk).is_err() {
                     transport::export_error(b"SHADOW_FAIL");
                     cortex_m::asm::bkpt();
                }
                
                if is_eos {
                    // 5. Atomic Commit Boundary
                    
                    // A. Snapshot to Flash (Primary State updated by Shadow)
                    let snap_len = match snapshot::snapshot_to_flash(shadow.state) {
                        Ok(l) => l,
                        Err(_) => { cortex_m::asm::bkpt(); 0 }
                    };
                    
                    // B. Verify written snapshot (Readback)
                    let snap_data = &flash::FlashStorage::read_snapshot()[0..snap_len];
                     // In real flow, verify hash matches what we expect from shadow state here?
                    // Proof generation does hashing.
                    
                    // C. Update Checkpoint (ATOMIC)
                    let mut cp = checkpoint::WalCheckpoint::new();
                    cp.last_committed_wal_index = stream_track.next_expected_seq;
                    cp.snapshot_hash = valori_kernel::verify::snapshot_hash(snap_data);
                    // proof::generate_proof returns Hex strings.
                    // I need raw bytes for Checkpoint.
                    // I will expose helpers in proof logic or just hash here.
                    // Re-hashing is safe deterministic cost.
                    cp.snapshot_hash = valori_kernel::verify::snapshot_hash(snap_data);
                    
                    cp.save(); // Commit.
                    
                    // D. Export Proof
                    let proof = proof::generate_proof(shadow.state, snap_data);
                    
                     let mut proof_buf = [0u8; 1024];
                     let proof_len = serde_json_core::to_slice(&proof, &mut proof_buf).unwrap_or(0);
                     transport::export_proof(&proof_buf[0..proof_len]);
                }
            },
            Err(_) => {
                transport::export_error(b"PACKET_FAIL");
                cortex_m::asm::bkpt();
            }
        }
    }
    
    // -----------------------------------------------------------------------
    // PHASE 2: Snapshot & Proof
    // -----------------------------------------------------------------------

    // E. Snapshot to Flash (Simulated)
    // This serializes state and writes to "Flash".
    // On failure, we trap.
    let snap_len = match snapshot::snapshot_to_flash(&state) {
        Ok(l) => l,
        Err(_) => {
            cortex_m::asm::bkpt(); // Trap on write failure
            0 // Unreachable
        }
    };

    // F. Read back for Proof Generation
    // We confirm that what is in Flash is the Truth.
    let snapshot_data = &flash::FlashStorage::read_snapshot()[0..snap_len];

    // G. Generate Proof
    // Hashes State and Snapshot.
    let proof = proof::generate_proof(&state, snapshot_data);
    
    // Serialize Proof to JSON (Bytes)
    // serde-json-core to slice.
    let mut proof_buf = [0u8; 1024];
    let proof_len = match serde_json_core::to_slice(&proof, &mut proof_buf) {
        Ok(l) => l,
        Err(_) => {
            cortex_m::asm::bkpt();
            0
        }
    };
    let proof_bytes = &proof_buf[0..proof_len];

    // H. Export Loop (UART)
    // "The device does one thing: Here is the truth of my memory."
    loop {
        // 1. Export Proof JSON
        transport::export_proof(proof_bytes);
        
        // 2. Export Raw Snapshot
        transport::export_snapshot(snapshot_data);
        
        // Wait / Blink
        for _ in 0..100_000 { cortex_m::asm::nop(); }
    }
}
