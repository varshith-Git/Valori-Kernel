# Valori System Invariants

These invariants are non-negotiable. Breaking any one of them compromises
correctness, auditability, or determinism. Every PR that touches affected
code paths must verify the relevant invariants are still satisfied.

Cross-reference: term definitions live in [`rfcs/0000-glossary.md`](rfcs/0000-glossary.md).

---

## I-01 — Operations are immutable

An `Operation` is never mutated after creation. If the user's intent changes,
a new `Operation` with a new `OperationId` and `OperationHash` is created.
The old `Operation` and any `Receipt`s it produced remain in history unchanged.

**Affected code:** `valori-planner`, `valori-metadata` (Operation store)

---

## I-02 — OperationHash is content-addressed

`OperationHash = BLAKE3(kind ‖ inputs ‖ policy)`.

The hash must be computed before planning begins and must not include any
Planner-derived fields (`PlannerFingerprint`, `PlanningContextHash`, `graph_hash`).
Two Operations with equal hashes are semantically identical and may share a
cached `ExecutionGraph` when the other cache key components also match.

**Affected code:** `valori-planner` (hash computation), `valori-metadata` (cache lookup)

---

## I-03 — ExecutionGraph is deterministic for a given triple

For the same `(OperationHash, PlannerFingerprint.hash, PlanningContextHash)` triple,
the Planner always produces the same `ExecutionGraph`, with the same tasks,
same edges, same `graph_hash`.

Non-determinism in planning (random IDs, timestamps) is a bug. `PlanningContext`
must be a deterministically serializable typed struct with no `HashMap<String, Value>`.

**Affected code:** `valori-planner`

---

## I-04 — Only the Kernel emits KernelEvents

`KernelEvent`s are produced exclusively by `KernelState::apply_event_ns()` inside
`valori-kernel`. No other crate may construct a `KernelEvent` and write it directly
to the audit log. The correct path is always:
`KernelCommand → [EventCommitter | Raft state machine] → KernelState::apply → KernelEvent → audit log`.

**Affected code:** `valori-kernel`, `valori-consensus` (`ValoriStateMachine`), `valori-node` (standalone engine)

---

## I-05 — Tasks never access KernelState directly

A Task communicates with the kernel exclusively by emitting `KernelCommand` Effects.
The `EffectBus` routes these to the appropriate engine. A Task that holds a reference
to `KernelState` or `SharedEngine` is a design violation.

**Affected code:** `valori-node` (task implementations)

---

## I-06 — Effects are the only communication between Tasks and external subsystems

All side effects (kernel writes, metric emissions, notifications, receipt fragments,
storage writes) must be expressed as typed `Effect` variants routed through the
`EffectBus`. Bypassing the `EffectBus` (e.g. direct `engine.write()` from a task)
breaks deduplication, receipt assembly, and backpressure.

**Affected code:** `valori-node` (all task handlers)

---

## I-07 — All KernelCommands from one Task target the same shard

A single Task may not split its mutations across two shards. If a logical operation
requires mutations on multiple shards, it must be expressed as multiple Tasks in the
`ExecutionGraph`, each targeting one shard. This is what makes one Task = one atomic
transaction.

**Affected code:** `valori-planner` (graph construction), `valori-node` (task dispatch)

---

## I-08 — One Task = one atomic transaction

From the kernel's perspective, all `KernelCommand`s emitted by a Task are applied
atomically via the shadow-first commit barrier (`EventCommitter`): shadow-apply → live-apply → persist.
A Task either completes fully or rolls back fully. There is no partial application.

**Affected code:** `valori-storage` (`EventCommitter`), `valori-consensus` (`ValoriStateMachine`)

---

## I-09 — Receipts prove execution, not correctness

A `Receipt` is cryptographic evidence that a specific `ExecutionGraph` ran under a
specific `KernelABI` and `PlannerFingerprint`, producing specific `state_hash_before`
and `state_hash_after` values. It does not certify that the output is semantically
correct (e.g. that an LLM answer is true). Do not conflate proof of execution with
proof of correctness.

**Affected code:** receipt consumers, documentation

---

## I-10 — Receipt assembly uses topological order, not completion order

`ReceiptFragment`s are sorted by their position in the `ExecutionGraph`'s topological
order before being hashed into the `Receipt`. Two independent replays of the same
`ExecutionGraph` must produce byte-identical receipts, regardless of which tasks
finished first at runtime. Using wall-clock completion order is a bug.

**Affected code:** `ReceiptAssembler`

---

## I-11 — KernelState is reconstructable from KernelSnapshot + KernelEvents

`KernelState` has exactly one reconstruction path:
1. Load `KernelSnapshot` (or start from empty state).
2. Replay every `KernelEvent` in the audit log that follows the snapshot.

No other path is valid. Reading raw storage files to synthesize state is prohibited.

**Affected code:** `valori-storage` (recovery), `valori-node` (bootstrap), `valori-consensus` (SM restore)

---

## I-12 — Every public type crossing crate boundaries is versioned

Any struct or enum that is serialized to disk, sent over the network, or included
in a `Receipt` must carry an explicit version field or be wrapped in a versioned
envelope. Implicit versioning via Rust type evolution is not sufficient.

**Affected code:** `valori-wire`, `valori-kernel` (snapshot format), `valori-storage` (event log format)

---

## I-13 — Durable Effects are acknowledged before task completion

A Task is not marked complete until every `EffectDurability::Durable` Effect it
emitted has received an acknowledgement from its target subsystem. `Ephemeral`
Effects (metrics, notifications) may be dropped without blocking completion.

**Affected code:** `valori-node` (task lifecycle), `EffectBus`

---

## I-14 — EffectId is unique per emission; EffectBus deduplicates

Every `Effect` emission generates a new `EffectId` (ULID). The `EffectBus`
records each `EffectId` it has dispatched. If a retried Task re-emits the same
logical Effect (same `EffectId`), the `EffectBus` drops the duplicate before
dispatch. This makes task retries idempotent at the effect level.

**Affected code:** `EffectBus`, task retry logic

---

## I-15 — KernelABI changes only on wire format changes

The `KernelABI` version increments only when the binary wire format of
`KernelEvent`s or the serialized `KernelState` changes in a way that breaks
backward compatibility. Pure internal refactors, performance improvements, or
new in-memory data structures that do not affect serialization do not change the ABI.
The current ABI version is published in `valori-kernel/src/lib.rs` as `KERNEL_ABI_VERSION`.

**Affected code:** `valori-kernel` (snapshot encode/decode), `valori-storage` (event log format)

---

## Enforcement

Each invariant is tagged with the crates it governs. When reviewing a PR that
touches those crates, explicitly check the invariant. If a test does not exist
that would catch a violation, add one.

Invariants I-04, I-08, I-10, I-11, and I-15 are testable via existing or
planned integration tests. Invariants I-01, I-03, I-07, I-09, I-12, I-13,
and I-14 require design review in addition to tests.
