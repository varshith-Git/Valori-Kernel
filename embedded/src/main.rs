#![no_std]
#![no_main]

// Valori embedded firmware — Cortex-M4
//
// Proves the Valori kernel executes deterministically on microcontrollers.
// Same KernelEvent log → same BLAKE3 state hash as a cloud node or laptop.
//
// Modes:
//   SelfTest  — inserts one hardcoded vector, snapshots, emits proof over UART.
//   WalReplay — continuous UART receive loop: ingest WAL packets → shadow-apply
//               → on EOS: commit checkpoint + emit proof → ready for next packet.
//               Also handles interleaved TYPE_SEARCH packets at any time.

extern crate alloc;

mod flash;
mod snapshot;
mod proof;
mod transport;
mod wal;
mod checkpoint;
mod wal_stream;
mod shadow;
mod recovery;
mod search;
mod inference; // INT matmul_engine integration

use cortex_m_rt::entry;
use embedded_alloc::Heap;
use panic_halt as _;

use valori_kernel::state::kernel::KernelState;
use valori_kernel::event::KernelEvent;
use valori_kernel::types::vector::FxpVector;
use valori_kernel::types::scalar::FxpScalar;
use valori_kernel::types::id::{RecordId, DEFAULT_NS};

// Vector dimension — must match VALORI_DIM on the cloud node for cross-platform
// proof verification. Change this to match your deployment's VALORI_DIM.
pub(crate) const DIM: usize = 128;

// Receive buffer large enough for one search request payload (3 + DIM*4 = 515 bytes)
// or one WAL packet payload.
const RX_PACKET_BUF: usize = 4096;

#[global_allocator]
static HEAP: Heap = Heap::empty();

// 192 KB heap — required for QGPTModel<61,64,64,256,4,3> (~172 KB) + KernelState.
// STM32F407 has 192 KB SRAM; RP2040 / nRF52840 have 256 KB and are also fine.
// If you need more headroom for KernelState records, shrink LAYERS or DIM in
// inference.rs (the 2-layer DIM-32 model needs only ~56 KB).
static mut HEAP_MEM: [u32; 49152] = [0; 49152]; // 49152 × 4 = 196 608 bytes

// Static receive buffer in .bss — not on the heap — keeps heap free for kernel data.
static mut PKT_BUF: [u8; RX_PACKET_BUF] = [0u8; RX_PACKET_BUF];

#[derive(PartialEq)]
enum BootMode {
    SelfTest,
    WalReplay,
}

const MODE: BootMode = BootMode::WalReplay;

#[entry]
fn main() -> ! {
    unsafe {
        let ptr = core::ptr::addr_of_mut!(HEAP_MEM);
        HEAP.init(ptr as usize, 196_608); // 192 KB
    }

    // Load the baked INT model from flash into the heap.
    // If the .bin is missing or malformed this returns false and inference
    // requests will reply with INFER_FAIL — the Valori WAL path keeps working.
    if !inference::init() {
        // Non-fatal: emit a warning and continue; search + WAL still operate.
        transport::export_error(b"INT_INIT_FAIL");
    }

    let mut state = KernelState::new();

    if MODE == BootMode::SelfTest {
        run_self_test(&mut state);
    } else {
        run_wal_replay(&mut state);
    }
}

// ── SelfTest mode ─────────────────────────────────────────────────────────────

fn run_self_test(state: &mut KernelState) -> ! {
    // Insert one deterministic test vector: 1.0 at dim-0, -1.0 at dim-2, 0.5 at dim-3.
    let mut vector = FxpVector::new_zeros(DIM);
    vector.data[0] = FxpScalar(65536);   // 1.0
    vector.data[2] = FxpScalar(-65536);  // -1.0
    vector.data[3] = FxpScalar(32768);   // 0.5

    let evt = KernelEvent::InsertRecord {
        id: RecordId(0),
        vector,
        metadata: None,
        tag: 0,
    };

    match state.apply_event_ns(&evt, DEFAULT_NS.0) {
        Ok(_) => {}
        Err(_) => cortex_m::asm::bkpt(),
    }

    emit_proof_loop(state)
}

/// Snapshot, generate proof, loop forever emitting over UART.
fn emit_proof_loop(state: &mut KernelState) -> ! {
    let snap_len = match snapshot::snapshot_to_flash(state) {
        Ok(l) => l,
        Err(_) => { cortex_m::asm::bkpt(); 0 }
    };

    let snapshot_data = &flash::FlashStorage::read_snapshot()[0..snap_len];
    let proof = proof::generate_proof(state, snapshot_data);

    let mut proof_buf = [0u8; 1024];
    let proof_len = serde_json_core::to_slice(&proof, &mut proof_buf).unwrap_or(0);

    loop {
        transport::export_proof(&proof_buf[0..proof_len]);
        transport::export_snapshot(snapshot_data);
        for _ in 0..100_000 { cortex_m::asm::nop(); }
    }
}

// ── WalReplay mode — continuous receive + dispatch loop ────────────────────────

fn run_wal_replay(state: &mut KernelState) -> ! {
    let last_seq = match recovery::recover(state) {
        Ok(seq) => seq,
        Err(_) => { cortex_m::asm::bkpt(); 0 }
    };

    let mut stream = wal_stream::WalStream::new(last_seq);
    let mut rx = transport::RxBuf::new();

    loop {
        let (kind, pkt_len) = loop {
            let buf = unsafe { &mut *(&raw mut PKT_BUF) };
            match transport::recv_packet(&mut rx, buf) {
                Ok(p)                          => break (p.kind, p.len),
                Err(transport::RecvError::BadSync) => {
                    transport::export_error(b"BADSYNC");
                    continue;
                }
                Err(transport::RecvError::Overflow) => {
                    transport::export_error(b"OVERFLOW");
                    cortex_m::asm::bkpt();
                }
            }
        };

        let pkt = unsafe { &PKT_BUF[0..pkt_len] };

        match kind {
            // ── WAL packet ────────────────────────────────────────────────
            transport::PacketKind::Wal => {
                match stream.ingest_packet(pkt) {
                    Err(_) => {
                        transport::export_error(b"SEQ_ERR");
                        cortex_m::asm::bkpt();
                    }
                    Ok((payload, is_eos)) => {
                        let mut shadow = shadow::ShadowKernel::new(state);
                        shadow.start_segment();

                        if shadow.apply_chunk(payload).is_err() {
                            transport::export_error(b"SHADOW_FAIL");
                            cortex_m::asm::bkpt();
                        }

                        if is_eos {
                            commit_and_emit_proof(shadow.state, &mut stream);
                        }
                    }
                }
            }

            // ── Search packet ─────────────────────────────────────────────
            // The host can send a search request at any time — even between
            // WAL segments — and the device answers against committed state.
            transport::PacketKind::Search => {
                search::handle(state, pkt);
            }

            // ── Inference packet ──────────────────────────────────────────
            // Run INT inference, store the output embedding + BLAKE3 receipt
            // into Valori's KernelState, reply with tokens + Valori proof.
            // The proof binds this inference into the same BLAKE3 audit chain
            // as all vector store operations — one chain proves everything.
            transport::PacketKind::Infer => {
                inference::handle(state, pkt);
            }

            transport::PacketKind::Unknown => {
                // Discard silently — forward compatibility.
            }
        }
    }
}

// ── Commit helper ─────────────────────────────────────────────────────────────

fn commit_and_emit_proof(state: &mut KernelState, stream: &mut wal_stream::WalStream) {
    let snap_len = match snapshot::snapshot_to_flash(state) {
        Ok(l) => l,
        Err(_) => { cortex_m::asm::bkpt(); 0 }
    };

    let snap_data = &flash::FlashStorage::read_snapshot()[0..snap_len];

    // Atomic commit point — power-loss before this line replays from the
    // previous checkpoint; after this line the new state is durable.
    let mut cp = checkpoint::WalCheckpoint::new();
    cp.last_committed_wal_index = stream.next_expected_seq;
    cp.snapshot_hash = valori_kernel::verify::snapshot_hash(snap_data);
    cp.save();

    let proof = proof::generate_proof(state, snap_data);
    let mut proof_buf = [0u8; 1024];
    let proof_len = serde_json_core::to_slice(&proof, &mut proof_buf).unwrap_or(0);
    transport::export_proof(&proof_buf[0..proof_len]);
}
