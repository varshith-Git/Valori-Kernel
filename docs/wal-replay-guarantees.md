# WAL Replay Guarantees

Valori's Write-Ahead Log (WAL) provides **deterministic crash recovery** with mathematical guarantees.

## Core Guarantee

Given:
- Initial state `S₀`
- Command log `WAL = [C₁, C₂, ..., Cₙ]`

Then:
```
Apply(S₀, WAL) on Device A = Apply(S₀, WAL) on Device B
```

**For ALL architectures** (x86, ARM, WASM, RISC-V, etc.)

This is proven via cryptographic hash comparison.

---

## Formal Specification

### State Transition Function

```
State(n+1) = Apply(State(n), Command(n))
```

Where:
- `Apply()` is deterministic (no randomness, no timestamps)
- `Command` is serialized via bincode (deterministic encoding)
- All math uses Q16.16 fixed-point (no floating point)

### Recovery Operation

```
S_recovered = Restore(snapshot) + Replay(WAL)

hash(S_recovered) ≡ hash(S_original)
```

**Equality is cryptographic** (BLAKE3 hash collision resistance: 2^128)

---

## WAL Format

Each WAL entry:
```
[Version: u8][Length: u32 LE][Command: bincode bytes]
```

- **Version**: WAL format version (currently `0x01`)
- **Length**: Command size in bytes (little-endian u32)
- **Command**: Deterministically serialized command

### Command Types

All commands are deterministic:
```rust
enum Command {
    InsertRecord { id: RecordId, vector: FxpVector },
    DeleteRecord { id: RecordId },
    CreateNode { node_id: NodeId, kind: NodeKind, record: Option<RecordId> },
    CreateEdge { edge_id: EdgeId, kind: EdgeKind, from: NodeId, to: NodeId },
    DeleteNode { node_id: NodeId },
    DeleteEdge { edge_id: EdgeId },
}
```

No timestamps, no UUIDs, no system calls.

---

## Recovery Procedure

### 1. Snapshot Restore
```rust
let snapshot_bytes = fs::read("state.snapshot")?;
let mut kernel = decode_state(&snapshot_bytes)?;
```

### 2. WAL Replay
``rust
let wal_reader = WalReader::open("commands.wal")?;

for cmd in wal_reader.commands() {
    kernel.apply(&cmd?)?;
}
```

### 3. Verification
```rust
let recovered_hash = kernel_state_hash(&kernel);
assert_eq!(recovered_hash, expected_hash);
```

---

## Failure Modes & Handling

### Incomplete WAL Entry

**Scenario**: Power loss mid-write

**Behavior**: 
- Reader detects incomplete length prefix or command data
- Returns `WalReaderError::Incomplete`
- Recovery stops at last complete command

**Result**: Consistent state (no partial updates applied)

### Corrupted WAL Data

**Scenario**: Bit flip in storage

**Behavior**:
- Bincode deserialization fails
- Returns `WalReaderError::Deserialization`
- Recovery aborts

**Mitigation**: 
- Use checksums (future enhancement)
- Hardware ECC memory
- Redundant storage

### WAL-Snapshot Mismatch

**Scenario**: WAL and snapshot from different timelines

**Behavior**:
- Commands reference non-existent IDs
- `kernel.apply()` returns `KernelError::NotFound`
- Recovery fails gracefully

**Prevention**:
- Atomic snapshot + WAL rotation
- Snapshot metadata includes last WAL sequence number

---

## Replay Symmetry

**Critical property**: WAL replay is **restart-symmetric**

```
Replay(WAL[0..50]) + crash + Resume(WAL[50..100])
  ≡
Replay(WAL[0..100])
```

This enables:
- Streaming WAL application
- Checkpoint interruption
- Incremental replication

Implementation: `valori-embedded/src/shadow.rs`

---

## Performance Characteristics

**WAL Write**:
- Serialization: ~1µs per command (16-dim insert)
- fsync: ~1-10ms (depends on storage)

**WAL Replay**:
- Deserialization: ~1µs per command
- Apply to kernel: ~5µs per command
- Total: ~6µs per command

**100 commands**: ~600µs replay time

---

## Production Recommendations

### Snapshot Frequency

**Guideline**: Snapshot every N commands where `N * replay_time < acceptable_downtime`

Example:
- Acceptable downtime: 100ms
- Replay time: 6µs/cmd
- Max commands: 100ms / 6µs ≈ 16,000 commands

**Recommendation**: Snapshot every 10,000 commands (leaves 40ms margin).

### WAL Rotation

After taking snapshot:
1. **Atomic rename**: `commands.wal` → `commands.wal.old`
2. **Create new WAL**: Fresh `commands.wal`
3. **Persist snapshot hash**: Link snapshot to WAL epoch

### Replication

For read-replicas:
1. Stream WAL to followers
2. Followers apply commands incrementally
3. Periodic snapshot sync for catchup

---

## Example: Embedded Crash Recovery

**Setup**:
```rust
let config = NodeConfig {
    snapshot_path: Some("state.snapshot"),
    wal_path: Some("commands.wal"),
    ..Default::default()
};

let mut engine = Engine::new(&config);
```

**Normal operation**:
```rust
// Inserts automatically go to WAL
for embedding in sensor_data {
    engine.insert_record_from_f32(&embedding)?;
}

// Periodic snapshot
engine.save_snapshot()?;
```

**After crash**:
```rust
let snapshot = fs::read("state.snapshot")?;
let mut engine = Engine::new(&config);

// Automatic recovery
let cmds_replayed = engine.restore_with_wal_replay(&snapshot, "commands.wal")?;

println!("Recovered! Replayed {} commands", cmds_replayed);
```

---

## Determinism Proof

**Test case** (included in CI):
```rust
// Device A
kernel_a.apply(&cmd)?;
let hash_a = kernel_state_hash(&kernel_a);

// Device B (different architecture)
kernel_b.apply(&cmd)?;
let hash_b = kernel_state_hash(&kernel_b);

assert_eq!(hash_a, hash_b); // ✅ Passes on x86, ARM, WASM
```

See [Multi-Arch Determinism](./multi-arch-determinism.md) for automated CI proof.

---

## Guarantees Summary

| Property | Guarantee |
|----------|-----------|
| **Determinism** | ✅ Bit-identical replay across architectures |
| **Atomicity** | ✅ No partial command application |
| **Durability** | ✅ fsync after each write |
| **Ordering** | ✅ Sequential replay preserves causality |
| **Restart Symmetry** | ✅ Interrupted replay ≡ full replay |

---

## Next Steps

- [Embedded Quickstart](./embedded-quickstart.md) - Get started on ARM Cortex-M
- [Deterministic Proof Format](./deterministic-proof.md) - Export verification proofs
- [Architecture](../architecture.md) - Deep dive into kernel design
