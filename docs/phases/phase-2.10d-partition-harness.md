# Phase 2.10d — Partition Harness

## Goal

Implement a switchable in-process network partition simulator for the 3-node Raft cluster and use it to verify the two properties that Phase 2.8 (process-kill tests) explicitly deferred: **asymmetric partition handling** and **BLAKE3 audit-chain consistency across a partition-and-heal cycle**.

## Delivered

### `crates/valori-consensus/src/partition_harness.rs` (pre-existing; extended with built-in tests)

The module was already wired but the gaps it left open were:

| Capability | Status before | Status after |
|---|---|---|
| `PartitionTable` — per-link block/unblock | ✅ shipped | (unchanged) |
| `make_cluster` / `wait_for_leader` / `wait_for_convergence` helpers | ✅ shipped | (unchanged) |
| Basic consensus + symmetric isolation tests | ✅ 4 tests | (unchanged) |
| **Asymmetric partition (one-directional block)** | ❌ gap | ✅ new test |
| **BLAKE3 hash frozen during partition, identical after heal** | ❌ gap | ✅ new tests |

### `crates/valori-consensus/tests/partition_scenarios.rs` (new)

Three tests covering the remaining scenarios:

1. **`asymmetric_partition_lagging_node_catches_up`**  
   Blocks only `leader → follower1` while the reverse link stays open. The 2/3 quorum (leader + follower2) commits 5 writes. After `unblock`, the lagging node must catch up; all 3 BLAKE3 hashes must be identical.

2. **`blake3_chain_consistent_across_partition_and_heal`**  
   Full compliance proof:  
   - 3 pre-partition writes, all 3 nodes converge.  
   - Symmetric isolation of the old leader; new leader elected on the surviving 2-node majority.  
   - 3 more writes committed through the new leader while the old leader is cut off.  
   - Assert: isolated node's hash is **frozen** at the pre-partition value during isolation.  
   - Heal; all 3 nodes must have all 6 records and identical BLAKE3 hashes.

3. **`isolated_node_hash_frozen_then_converges`**  
   Isolates a follower after 2 baseline writes. 3 more writes committed on the 2/3 majority. Isolated node's hash must be frozen (no divergence). After heal, all 3 nodes share 5 records and one hash.

## Findings

1. **openraft's `AppendEntries` is the only path that needs blocking for partition simulation.** Because the partition harness returns `RPCError::Network` synchronously (no real socket), blocked RPCs fail instantly rather than timing out — election re-triggering happens in ~150 ms (the `election_timeout_min`), far faster than real hardware.

2. **The hash-frozen assertion required a deliberate `tokio::time::sleep(200 ms)` delay** after committing the post-partition writes, to give the runtime enough time to replicate to the non-isolated nodes while confirming no replication leaked to the isolated node. This is an inherent race between "did the non-isolated nodes get the entry?" and "did any stray message reach the isolated node?" The 200 ms margin is safe given 50 ms heartbeat + 150 ms election timeout configured in `make_cluster`.

3. **No change to the transport or state machine was needed.** The `PartitionTable` + `PartitionNetworkFactory` design (one network factory per node, `check_partition` before every RPC) was already the correct architecture.

## Validation

```
cargo test -p valori-consensus --test partition_scenarios
```

```
running 3 tests
test asymmetric_partition_lagging_node_catches_up ... ok
test isolated_node_hash_frozen_then_converges ... ok
test blake3_chain_consistent_across_partition_and_heal ... ok

test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.73s
```

Full suite (all `valori-consensus` tests):

```
cargo test -p valori-consensus
```

All tests pass (no failures across fault_tolerance, partition_scenarios, grpc_cluster, snapshot_transfer, proptest_event_fuzz, log_store, and type_config test files).

## Follow-ups

- **Mid-snapshot-transfer leader change**: partition the leader while it is streaming a snapshot to a latecomer and verify the latecomer receives the snapshot from the new leader. Requires `max_in_snapshot_log_to_keep = 0` config (already used in Phase 2.7) and a third node joining after many log entries have been purged. Could land in a future hardening phase.
- **5-node partition scenarios**: minority-of-2 vs majority-of-3 requires a 5-node cluster. `make_cluster(5)` works today; tests using it are not yet written.
