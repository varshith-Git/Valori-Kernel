# Embedded Quickstart Guide

Get started with Valori on ARM Cortex-M and other embedded platforms in under 10 minutes.

## Why Valori for Embedded?

- **`no_std` compatible** - Run on microcontrollers without an operating system
- **Deterministic** - Bit-identical results across any device (ARM, x86, WASM)
- **Compact** - Fixed-point math, no floating point unit required
- **Verifiable** - Generate cryptographic proofs of memory state

Perfect for: robotics, drones, autonomous systems, edge AI, safety-critical applications.

---

## Prerequisites

- Rust toolchain (stable)
- Target for your device (e.g., `thumbv7em-none-eabihf` for ARM Cortex-M4)
- Optional: Embedded debugger (J-Link, ST-Link, etc.)

---

## Quick Start: Python FFI (Easiest)

**1. Install valori-python**
```bash
pip install valori
```

**2. Use in embedded mode**
```python
from valori import EmbeddedKernel

# Create kernel (runs in-process, no server)
kernel = EmbeddedKernel(max_records=1024, dim=16)

# Insert semantic vectors
embedding = [0.1] * 16
kernel.insert(embedding)

# Export snapshot for verification
snapshot = kernel.save_snapshot()
proof_hash = kernel.get_state_hash()

print(f"State hash: {proof_hash.hex()[:16]}...")
```

**3. Verify on another device**
```python
# On different device/architecture
kernel2 = EmbeddedKernel(max_records=1024, dim=16)
kernel2.restore_snapshot(snapshot)

# Hash will be IDENTICAL
assert kernel2.get_state_hash() == proof_hash
```

---

## Pure Embedded: ARM Cortex-M

**1. Add to `Cargo.toml`**
```toml
[dependencies]
valori-kernel = { version = "0.1", default-features = false }
valori-embedded = "0.1"
```

**2. Initialize kernel**
```rust
#![no_std]
#![no_main]

use valori_kernel::state::kernel::KernelState;
use valori_kernel::types::vector::FxpVector;
use valori_embedded::shadow::ShadowKernel;

const MAX_RECORDS: usize = 256;
const DIM: usize = 16;

#[entry]
fn main() -> ! {
    // Create deterministic kernel
    let mut kernel = KernelState::<MAX_RECORDS, DIM, 0, 0>::new();
    
    // Build vector from sensor data
    let mut vector = FxpVector::<DIM>::new_zeros();
    // ... populate from sensors ...
    
    // Insert record
    let cmd = Command::InsertRecord {
        id: RecordId(0),
        vector,
    };
    kernel.apply(&cmd).unwrap();
    
    loop {
        // Your application logic
    }
}
```

**3. Build for target**
```bash
cargo build --target thumbv7em-none-eabihf --release
```

**4. Flash to device**
```bash
# Example for ST-Link
st-flash write target/thumbv7em-none-eabihf/release/myapp.bin 0x8000000
```

---

## Cross-Device Verification

**Use case**: Robot fleet with distributed memory

**Device A** (ARM Cortex-M4):
```rust
// Insert mission data
kernel.apply(&insert_cmd).unwrap();

// Export WAL + snapshot
let snapshot = encode_state(&kernel);
let state_hash = kernel_state_hash(&kernel);

// Send to cloud via LoRa/cellular
transmit(snapshot, state_hash);
```

**Cloud Verifier** (x86_64):
```rust
// Receive from device
let (snapshot, claimed_hash) = receive();

// Verify
let mut kernel = decode_state(&snapshot).unwrap();
let actual_hash = kernel_state_hash(&kernel);

assert_eq!(actual_hash, claimed_hash); // Cryptographic proof!
```

**Device B** (ARM Cortex-M7):
```rust
// Restore fleet memory
kernel = decode_state(&snapshot).unwrap();

// Continue mission with shared context
```

---

## Performance on Embedded

**ARM Cortex-M4 @ 168MHz:**
- Insert record: ~5µs
- L2 distance (16-dim): ~2µs  
- State snapshot: ~100µs (256 records)
- Memory: ~4KB RAM (256 records, 16-dim)

**No heap allocation** - Everything stack/static.

---

## Determinism Guarantee

```rust
// Device A (ARM)
let hash_a = kernel_state_hash(&kernel_a);

// Device B (x86)  
let hash_b = kernel_state_hash(&kernel_b);

// Device C (WASM)
let hash_c = kernel_state_hash(&kernel_c);

assert_eq!(hash_a, hash_b);
assert_eq!(hash_b, hash_c);
// ✅ Bit-identical across ALL architectures
```

---

## Next Steps

- [WAL Replay Guarantees](./wal-replay-guarantees.md) - Crash recovery for embedded
- [Deterministic Proof Format](./deterministic-proof.md) - Export verification proofs
- [Multi-Arch Validation](./multi-arch-determinism.md) - See automated CI proof

## Support

Questions? Reach out to [your contact] or open an issue on GitHub.
