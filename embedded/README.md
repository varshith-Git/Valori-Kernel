# valori-embedded

Cortex-M firmware proving the Valori kernel executes deterministically on
microcontrollers: same input commands → same BLAKE3 state hash as a cloud
node or a laptop. Validated on ARM Cortex-M4 @ 168 MHz.

## Building

This crate is `#![no_std]` + `#![no_main]` with its own panic handler — it
**cannot build for a host target** and is therefore excluded from the
workspace's `default-members`:

```bash
rustup target add thumbv7em-none-eabihf
cargo build -p valori-embedded --target thumbv7em-none-eabihf --release
```

## Layout

- `src/main.rs` — firmware entry, heap setup, command loop
- `src/flash.rs`, `src/wal.rs`, `src/snapshot.rs` — flash-backed persistence
- `src/shadow.rs`, `src/recovery.rs`, `src/proof.rs` — shadow execution,
  crash recovery, proof generation against the same kernel as every other
  deployment target
