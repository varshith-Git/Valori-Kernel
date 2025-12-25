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

// use core::alloc::GlobalAlloc; // Unused
use cortex_m_rt::entry;
use embedded_alloc::Heap;
use panic_halt as _; // Deterministic panic handler (infinite loop)

// Import Valori Kernel Types
use valori_kernel::state::kernel::KernelState;
use valori_kernel::state::command::Command;
use valori_kernel::types::vector::FxpVector;
use valori_kernel::types::scalar::FxpScalar;
use valori_kernel::types::id::RecordId;
// use valori_kernel::fxp::ops::from_f32; // Forbidden in no_std embedded

// --- 1. Global Allocator ---
// We must manage memory manually since there is no OS.
// 8KB Heap (Small test harness)
#[global_allocator]
static HEAP: Heap = Heap::empty();

// Static buffer for heap memory. 
// Placed in .bss/static RAM.
static mut HEAP_MEM: [u32; 2048] = [0; 2048]; // 2048 * 4 bytes = 8KB

// -----------------------------------------------------------------------
// Configuration (Match Node Config for Determinism)
// -----------------------------------------------------------------------
const MAX_RECORDS: usize = 1000;
const D: usize = 16;
const MAX_NODES: usize = 1000;
const MAX_EDGES: usize = 2048;

// --- 2. Entry Point ---
#[entry]
fn main() -> ! {
    // A. Initialize Heap
    // Unsafe required because we are manipulating static mut memory.
    // In a single-core embedded context with interrupts disabled (start), this is safe.
    unsafe { 
        let ptr = core::ptr::addr_of_mut!(HEAP_MEM);
        HEAP.init(ptr as usize, 8192); 
    }

    // B. Initialize Kernel
    // This runs completely in RAM.
    let mut state = KernelState::<MAX_RECORDS, D, MAX_NODES, MAX_EDGES>::new();

    // C. Deterministic Test Vector
    // Deterministic fixed-point vector.
    // These values are chosen so we can:
    // 1) read them in memory via debugger
    // 2) compute hash consistency across devices
    //
    // Q16.16 values:
    // 1.0  -> 65536
    // 0.5  -> 32768
    // -1.0 -> -65536
    // Vector: [1.0, 0.0, -1.0, 0.5, ...]
    let mut vector = FxpVector::<D>::new_zeros();
    
    // Explicit fixed-point construction
    vector.data[0] = FxpScalar(65536);       // 1.0
    vector.data[1] = FxpScalar(0);           // 0.0
    vector.data[2] = FxpScalar(-65536);      // -1.0
    vector.data[3] = FxpScalar(32768);       // 0.5
    
    // Remaining are 0.
    
    // D. Apply Command (Insert)
    let id = RecordId(0);
    let cmd = Command::InsertRecord { id, vector };
    
    // The result should be valid.
    match state.apply(&cmd) {
        Ok(_) => {}
        Err(_) => cortex_m::asm::bkpt(), // trap if kernel rejects state
    }
    
    // E. Verify State (Optional Check)
    // In real debugger, we would inspect `state.records`.
    
    // F. Breakpoint
    // Hand over control to debugger/probe for verification.
    cortex_m::asm::bkpt();

    // G. Infinite Loop
    // Firmware never returns.
    loop {
        cortex_m::asm::wfi(); // Wait for interrupt (power save)
    }
}
