# valori-embedded

Cortex-M firmware that runs the same `valori-kernel` as the cloud node — proving
that the same `KernelEvent` log produces the same BLAKE3 state hash on a
microcontroller, a laptop, or a server rack.

This is the **portability proof**: Valori is not a database, it is a deterministic
memory computer that can execute on any target with an allocator.

---

## How it works

```
Host (laptop / cloud node)
  │  frames KernelEvent as a WAL packet over UART
  ▼
Firmware receive loop
  │  shadow-applies the event (provisional, not yet committed)
  │  on EOS flag → snapshot to flash → save checkpoint → emit BLAKE3 proof
  ▼
Host verifies: proof.final_state_hash == node's /v1/proof hash
               ✓ same kernel, same events, same hash — on MCU
```

Search queries can arrive at any time between WAL packets. The device answers
against committed state and attaches the current state hash to the result so
the host can verify which snapshot was searched.

---

## Supported hardware targets

| Board | Chip | Target triple | Status |
|---|---|---|---|
| **Raspberry Pi Pico** | RP2040 (Cortex-M0+) | `thumbv6m-none-eabi` | Supported (`--features pico`) |
| **STM32F4 Discovery** | STM32F407 (Cortex-M4) | `thumbv7em-none-eabihf` | Supported (default) |
| **Arduino Nano 33 BLE** | nRF52840 (Cortex-M4) | `thumbv7em-none-eabihf` | UART addr change only |
| **QEMU lm3s6965evb** | Cortex-M3 (simulated) | `thumbv7em-none-eabihf` | `--features qemu` |

> Standard Arduinos (Uno, Mega, Nano classic) use AVR (8-bit) — **not compatible**.
> Raspberry Pi 1-5 run Linux — use `valori-node` there instead.

---

## Build

### Prerequisites

```bash
# Cortex-M4 target (STM32F4, Arduino Nano 33 BLE, QEMU)
rustup target add thumbv7em-none-eabihf

# Cortex-M0+ target (Raspberry Pi Pico)
rustup target add thumbv6m-none-eabi
```

### STM32F4 Discovery (default)

```bash
cargo build -p valori-embedded \
  --target thumbv7em-none-eabihf \
  --release \
  --features mcu
```

### Raspberry Pi Pico

```bash
cargo build -p valori-embedded \
  --target thumbv6m-none-eabi \
  --release \
  --features mcu,pico
```

### QEMU (lm3s6965evb simulation)

```bash
cargo build -p valori-embedded \
  --target thumbv7em-none-eabihf \
  --release \
  --features mcu,qemu
```

> The `mcu` feature is required for all firmware builds. It gates the
> `#![no_std]` binary so that `cargo test -p valori-embedded` can run
> the host-side determinism tests without the MCU deps interfering.

---

## Flashing to hardware

### Raspberry Pi Pico

**One-time setup:**
```bash
cargo install elf2uf2-rs   # converts ELF → UF2 flash format
```

**Add `.cargo/config.toml` inside `embedded/`:**
```toml
[build]
target = "thumbv6m-none-eabi"

[target.thumbv6m-none-eabi]
runner = "elf2uf2-rs --deploy --serial"
rustflags = ["-C", "link-arg=-Tlink.x"]
```

**Add `memory.x` inside `embedded/`:**
```
MEMORY {
    BOOT2 : ORIGIN = 0x10000000, LENGTH = 0x100
    FLASH : ORIGIN = 0x10000100, LENGTH = 2048K - 0x100
    RAM   : ORIGIN = 0x20000000, LENGTH = 264K
}
```

**Flash:**
1. Hold **BOOTSEL** on the Pico and plug USB into your laptop
2. Release BOOTSEL — a drive called `RPI-RP2` mounts
3. Run:

```bash
cargo run -p valori-embedded \
  --target thumbv6m-none-eabi \
  --release \
  --features mcu,pico
```

The Pico reboots and starts the firmware automatically.

---

### STM32F4 Discovery

The Discovery board has a built-in **ST-LINK** debugger — no extra hardware needed.

**One-time setup:**
```bash
cargo install probe-rs-tools --locked
# or: brew install probe-rs  (macOS)
```

**Add `.cargo/config.toml` inside `embedded/`:**
```toml
[build]
target = "thumbv7em-none-eabihf"

[target.thumbv7em-none-eabihf]
runner = "probe-rs run --chip STM32F407VGTx"
rustflags = ["-C", "link-arg=-Tlink.x"]
```

**Add `memory.x` inside `embedded/`:**
```
MEMORY {
    FLASH : ORIGIN = 0x08000000, LENGTH = 1024K
    RAM   : ORIGIN = 0x20000000, LENGTH = 128K
}
```

**Flash:**
```bash
# Plug board via USB — probe-rs detects the ST-LINK automatically
cargo run -p valori-embedded \
  --target thumbv7em-none-eabihf \
  --release \
  --features mcu
```

No button presses needed — `probe-rs` flashes and resets in one step.

---

### Arduino Nano 33 BLE

Same Cortex-M4 target as STM32F4. Only the UART register address differs
(nRF52840 UART0 DR = `0x4003_4000`). Add a `nrf` feature to `Cargo.toml` and
a matching `#[cfg(feature = "nrf")]` block in `transport.rs`, then follow the
[nrf-hal flashing guide](https://github.com/nrf-rs/nrf-hal).

---

## Verifying the firmware is running

Open the serial port after boot:

```bash
# macOS
screen /dev/tty.usbmodem* 115200

# Linux
minicom -D /dev/ttyACM0 -b 115200
```

In `SelfTest` mode the firmware immediately sends a framed `TYPE_PROOF` packet
(binary, starts with `0x55 0xAA 0x55 0xAA 0x01`). In `WalReplay` mode it
waits silently for incoming packets.

---

## Sending WAL packets from the host

Packet framing: `[SYNC:4][TYPE:1][LEN:4 LE][PAYLOAD]`

| Constant | Value | Direction |
|---|---|---|
| `TYPE_WAL` | `0x03` | host → device |
| `TYPE_SEARCH` | `0x04` | host → device |
| `TYPE_INFER` | `0x06` | host → device |
| `TYPE_PROOF` | `0x01` | device → host |
| `TYPE_SEARCH_RESULT` | `0x05` | device → host |
| `TYPE_INFER_RESULT` | `0x07` | device → host |
| `TYPE_ERR` | `0xEE` | device → host |
| Sync word | `0x55 0xAA 0x55 0xAA` | both directions |

### WAL packet payload

```
[WalHeader: 16 bytes]
  version:          u32 LE  (must be 1)
  encoding_version: u32 LE
  dim:              u32 LE  (must match firmware DIM = 128)
  checksum_len:     u32 LE

[KernelEvent: bincode-encoded, variable length]
```

Set `flags = 0x01` (FLAG_EOS) on the last packet of a segment to trigger
the atomic commit + proof emission.

### Search packet payload

```
[namespace_id: u16 LE]   (0 = default namespace)
[k:            u8]       (1–8 results wanted)
[query_scalar_0..127: each i32 LE, Q16.16 fixed-point]
```

Total: `3 + 128×4 = 515 bytes`

### Search result payload (device → host)

```
[k_found:      u8]
[version:      u64 LE]   kernel version at search time
[{id: u32 LE, score: u32 LE} × k_found]
[state_hash:   32 bytes]  BLAKE3 — verify against /v1/proof
```

### Inference packet payload (host → device, TYPE_INFER)

```
[gen_len:    u8]          number of tokens to generate (1–32)
[prompt_len: u8]          number of prompt token IDs
[tokens:     prompt_len × u8]   token IDs in [0, VOCAB)
```

### Inference result payload (device → host, TYPE_INFER_RESULT)

```
[ok:         u8]          1 = success, 0 = error
[out_len:    u8]          number of generated tokens
[tokens:     out_len × u8]
[receipt:    32 bytes]    BLAKE3(model_hash | prompt | output)
[record_id:  u32 LE]      RecordId assigned in KernelState
[version:    u64 LE]      KernelState version after insert
[state_hash: 32 bytes]    Valori BLAKE3 state hash after insert
```

The `receipt` is computed as `BLAKE3(model_hash || prompt_tokens || output_tokens)`.
A verifier can replay the same prompt through the same model binary, recompute the
receipt, and confirm it appears at this position in the Valori audit chain.

The `state_hash` returned here matches the cloud node's `/v1/proof` hash (assuming
both have received the same event log), enabling end-to-end proof across devices.

---

## On-device RAG (inference.rs)

`inference.rs` wires INT's `QGPTModel` into Valori's audit chain:

1. **Model baked into flash** — `tiny_transformer_int8.bin` is embedded via `include_bytes!`.
   The model BLAKE3 hash becomes part of every inference receipt.
2. **Greedy decode** — prefill + autoregressive decode, all Q8 integer math, no f32 on the hot path.
3. **Logit fingerprint stored in Valori** — the final-step logit distribution is converted to
   Q16.16 `FxpVector` and inserted into `KernelState` via `KernelEvent::InsertRecord`.
   The 32-byte BLAKE3 receipt goes into the `metadata` field.
4. **On-device RAG** — `handle_with_rag()` searches Valori for K nearest past inferences,
   prepends their tokens as context, then runs inference. The MCU retrieves from its own
   memory — no server needed.

**Memory budget (STM32F407, 192 KB RAM):**
```
QGPTModel<61,64,64,256,4,3>  ≈ 172 KB (heap)
KV caches                      ≈  24 KB (stack per request)
KernelState + WAL buffers      ≈  16 KB
─────────────────────────────────────
Total peak                     ≈ 212 KB  ← fits on STM32F407 with HEAP_MEM tuned
```

For RP2040 (264 KB) or nRF52840 (256 KB) this fits comfortably. For a smaller model
(2 layers, DIM=32), RAM footprint drops to ~56 KB, leaving plenty for the vector store.

---

## QEMU smoke test

```bash
./embedded/scripts/qemu_test.sh             # build + run under QEMU
./embedded/scripts/qemu_test.sh --build-only  # compile check only
```

Requires `qemu-system-arm` on PATH (`brew install qemu` / `apt install qemu-system-arm`).

---

## Host-side determinism tests

These run on your laptop (std target) and prove the core claim without hardware:

```bash
cargo test -p valori-embedded
```

**8 tests:**
- `same_events_produce_same_hash` — identical event log → identical hash (two independent runs)
- `empty_state_hash_is_stable` — empty kernel hash is deterministic
- `different_content_produces_different_hash` — vector values affect hash
- `snapshot_roundtrip_preserves_state_hash` — encode→decode preserves hash
- `snapshot_hash_is_stable` — snapshot bytes are deterministic
- `search_returns_inserted_vector_as_top1` — exact match returns score=0
- `search_result_paired_with_state_hash_is_verifiable` — search does not mutate state
- `self_test_hash_anchor` — prints the ground-truth hash; pin it against the cloud node's `/v1/proof` to complete the cross-platform verification

---

## Source layout

| File | Purpose |
|---|---|
| `src/main.rs` | Entry point — heap init, `SelfTest` / `WalReplay` dispatch |
| `src/transport.rs` | UART TX/RX ring buffer, framed packet send/receive, board UART addresses |
| `src/wal.rs` | WAL header parsing, bincode `KernelEvent` decode → `apply_event` |
| `src/wal_stream.rs` | Sequence-ordered packet framing, EOS detection |
| `src/shadow.rs` | Provisional (pre-commit) kernel execution + BLAKE3 accumulator |
| `src/snapshot.rs` | `encode_state` → simulated flash |
| `src/flash.rs` | Simulated flash storage (RAM buffer; replace with real HAL for production) |
| `src/checkpoint.rs` | Power-loss-safe WAL checkpoint (sequence + snapshot hash) |
| `src/recovery.rs` | Boot recovery: checkpoint → hash verify → snapshot restore |
| `src/proof.rs` | `EmbeddedProof` — `snapshot_hash` + `kernel_state_hash` → hex JSON |
| `src/search.rs` | Parse search request, call `search_l2_ns`, emit verifiable result |
| `src/inference.rs` | INT `QGPTModel` integration — greedy decode, BLAKE3 receipt, on-device RAG |
| `tests/cross_platform_hash.rs` | Host-side CI tests proving determinism claim |
| `scripts/qemu_test.sh` | QEMU build + smoke test script |

---

## Key constants (match these to your cloud node)

| Constant | Location | Must match |
|---|---|---|
| `DIM = 128` | `src/main.rs` | `VALORI_DIM` env var on the node |
| `MAX_K = 8` | `src/search.rs` | max k in search requests |
| `HEAP = 96 KB` | `src/main.rs` | must fit on target board RAM |
| Snapshot buffer `64 KB` | `src/snapshot.rs` | must fit in simulated flash |

---

## Features

| Feature | Effect |
|---|---|
| `mcu` | Required for the `#![no_std]` binary. Always set when cross-compiling. |
| `qemu` | Maps UART TX/RX to QEMU `lm3s6965evb` UART0 (`0x4000_C000`). |
| `pico` | Maps UART to RP2040 UART0 DR (`0x4003_4000`). Changes target to `thumbv6m-none-eabi`. |
