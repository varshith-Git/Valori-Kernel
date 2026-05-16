# Valori Architecture: Auxiliary Crates (CLI & Persistence)

In addition to the core `valori-kernel` and `valori_node`, the Valori workspace includes a `crates/` directory containing specialized utilities. Two of the most important are the **Persistence** and **CLI** crates.

---

## 1. The Persistence Crate (`crates/persistence/`)

**Location**: `crates/persistence/src/wal.rs` and `crates/persistence/src/snapshot.rs`

While `valori-kernel` defines *how* state is serialized into a byte array, the `valori_persistence` crate defines *where* and *how* those bytes are safely written to the host operating system's filesystem.

### `SnapshotHeader`
```rust
pub struct SnapshotHeader {
    pub magic: [u8; 4],         // "VALO"
    pub version: u32,           // 1
    pub event_index: u64,       // The sequence number of the last applied event
    pub timestamp: u64,         // Unix timestamp
    pub state_hash: [u8; 16],   // MD5 or partial hash
    pub reserved: [u8; 8],
}
```
This 48-byte header wraps the raw `[u8]` output of the `valori-kernel` snapshot encoder. It adds file-level metadata like the physical `timestamp` and the `event_index`, allowing the system to quickly scan snapshot files without having to decode the entire multi-gigabyte kernel payload.

### `WalEntryHeader` & `WalReader`
The Write-Ahead Log (WAL) appends events one by one. Each event gets a 20-byte header containing the `event_id`, the `payload_len`, and a `checksum` (CRC64).
- **Corruption Resilience**: If a server crashes mid-write, the `WalReader` iterator uses the checksum to detect the torn write. It will gracefully yield an `UnexpectedEof` or `ChecksumMismatch` error, stopping replay exactly at the last valid event and preserving deterministic state.

---

## 2. The Forensic CLI (`crates/cli/`)

**Location**: `crates/cli/src/main.rs` and `crates/cli/src/engine.rs`

Because Valori is perfectly deterministic and event-sourced, it is possible to build powerful debugging tools that are impossible in standard databases. The **Valori Forensic CLI** acts as a "Black Box Flight Recorder" for an AI agent's memory.

### Features
- **`Inspect`**: Scans a directory and verifies the integrity of the `snapshot.val` and `events.log` files.
- **`Timeline`**: Dumps a human-readable list of every semantic event that mutated the memory state.
- **`Diff`**: Compares the exact system state at Event A vs Event B.
- **`ReplayQuery`**: Allows developers to "time travel." 

### How Time Travel Works (`ForensicEngine`)
```rust
pub fn replay_to(&mut self, wal_path: &str, target_index: u64) -> Result<usize> { ... }
```
When a user wants to debug why an AI retrieved the wrong chunk of context on Tuesday at 4:00 PM, they can use `ReplayQuery`:
1. The CLI loads the closest prior Snapshot.
2. It opens the WAL stream and begins replaying events.
3. It **stops** replaying exactly at the `target_index` (the Event ID immediately prior to the AI's bad query).
4. Because the `valori-kernel` is mathematically deterministic, the CLI's memory layout is now **bit-for-bit identical** to the live server's memory layout at that exact microsecond on Tuesday.
5. The CLI then executes the semantic query locally, yielding the exact same distances, sorting orders, and tie-breakers that the live server produced. 

This enables true reproducible debugging for non-deterministic AI pipelines.
