# Embedded Quickstart Guide

Valori's kernel (`valori-kernel`) is `no_std` / `no_alloc` — it runs on ARM Cortex-M
and other microcontrollers with no operating system. The Python path (`MemoryClient`)
uses the same kernel in-process via PyO3 FFI.

---

## Why Valori for embedded?

- **`no_std`** — no OS, no heap required
- **Deterministic** — Q16.16 fixed-point, bit-identical on ARM / x86 / WASM
- **Verifiable** — BLAKE3 state hash provable without a server
- **Compact** — ~4 KB RAM for 256 records at dim=16

Perfect for: robotics, drones, edge AI, safety-critical applications.

---

## Python path (FFI, easiest)

No server needed — the Rust kernel runs inside your Python process.

```bash
pip install "valoricore[local]"
```

```python
from valoricore import MemoryClient
from valoricore.embeddings import SentenceTransformerEmbedder

embedder = SentenceTransformerEmbedder("all-MiniLM-L6-v2")
db = MemoryClient(path="./my_db", dim=384)

db.add_document(text="Sensor reading: 42.7 °C", embed=embedder)
hits = db.semantic_search("temperature", embed=embedder, k=3)

# Verify on any other machine that replayed the same events
print(db.get_state_hash())   # 64-char BLAKE3 hex
```

---

## Rust path — ARM Cortex-M4

**`Cargo.toml`:**
```toml
[dependencies]
valori-kernel = { path = "../crates/valori-kernel", default-features = false }
```

**`src/main.rs`:**
```rust
#![no_std]
#![no_main]

use valori_kernel::state::kernel::KernelState;
use valori_kernel::event::KernelEvent;
use valori_kernel::types::id::RecordId;
use valori_kernel::fxp::scalar::FxpScalar;

#[entry]
fn main() -> ! {
    let mut kernel = KernelState::new();

    // Insert a record from sensor data
    let vector: alloc::vec::Vec<FxpScalar> = (0..16)
        .map(|i| FxpScalar::from_f32(i as f32 * 0.1))
        .collect();
    let event = KernelEvent::InsertRecord {
        id: RecordId(0),
        vector,
        metadata: None,
        tag: 0,
    };
    kernel.apply_event_ns(event, 0).unwrap();

    loop { /* your application */ }
}
```

**Build:**
```bash
cargo build --target thumbv7em-none-eabihf --release
```

---

## Cross-device verification

```
Device A (ARM) ──── snapshot + state_hash ──→ Cloud verifier (x86)
                                               assert actual_hash == claimed_hash  ✓

Device B (ARM Cortex-M7) ←── restore snapshot ──
                              # Continues with shared memory
```

The state hash is the same on all three because Q16.16 integer math has no
floating-point variance — IEEE 754 rounding differences cannot occur.

---

## Verifying the `no_std` build in CI

```bash
cargo build -p valori-kernel --target wasm32-unknown-unknown
```

This is required by CLAUDE.md invariant #7 after any change to `valori-kernel`.

---

## Next steps

- [WAL replay guarantees](./wal-replay-guarantees.md)
- [Determinism proof format](./deterministic-proof.md)
- [Multi-arch CI validation](./multi-arch-determinism.md)
