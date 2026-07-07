# Valori Compatibility Matrix

This document defines the compatibility policy for every versioned boundary
in the Valori platform. It is the authoritative reference for upgrade,
migration, and multi-version cluster decisions.

Cross-reference: term definitions in [`rfcs/0000-glossary.md`](rfcs/0000-glossary.md),
system invariants in [`INVARIANTS.md`](INVARIANTS.md).

---

## Versioned boundaries

| Boundary | Version type | Where published | Who reads it |
|---|---|---|---|
| `KernelABI` | `semantic_version + event_schema_hash + state_schema_hash` | `valori-kernel/src/lib.rs` | `valori-planner`, receipt consumers, verifier |
| `PlannerFingerprint` | `BLAKE3(version ‖ routing_config ‖ feature_flags ‖ metadata_schema_version)` | computed at Planner startup | planner cache, `Receipt` |
| Snapshot format | integer version (`V5`, `V6`, …) | first byte of snapshot file | `valori-storage` (decode), `valori-node` (restore) |
| Event log format | `v2`/`v3` segment header | first byte of each segment | `valori-storage` (replay, splice verify) |
| Wire types (`valori-wire`) | semver crate version | `Cargo.toml` | Python SDK, CLI, HTTP clients |
| HTTP API | URL path prefix (`/v1/`, …) | route definitions in `server.rs` | Python SDK, UI, external callers |
| Raft log entries | `ClientRequest` struct version | `valori-consensus/src/types.rs` | all cluster nodes |

---

## KernelABI compatibility rules

### What changes the ABI

- Adding, removing, or reordering fields in any `KernelEvent` variant.
- Changing the binary encoding of `KernelState` (snapshot format version bump).
- Changing the BLAKE3 hashing domain for the state hash.

### What does NOT change the ABI

- New in-memory fields that are not serialized.
- New index structures (`HnswIndex`, `IvfIndex`) that rebuild from existing records.
- Algorithmic changes to search scoring that don't touch event or state serialization.
- Adding new `KernelEvent` variants without removing old ones (additive, backward-compatible).

### Migration requirement

When `KernelABI` changes in a breaking way:
1. Bump `KERNEL_ABI_VERSION` in `valori-kernel/src/lib.rs`.
2. Update snapshot decode in `valori-kernel/src/snapshot/decode.rs` to handle the old format.
3. Add a migration test in `valori-kernel/tests/snapshot_roundtrip.rs` that decodes an old-format snapshot.
4. Document the migration in this file under **Migration history** below.

---

## Snapshot format compatibility

| Version | Status | Decoder support |
|---|---|---|
| V5 | Legacy | Decoded by V6 decoder; records land in `DEFAULT_NS` (no namespace metadata). |
| V6 | Current | Native. Adds per-record `namespace_id`, `next_in_ns`, `prev_in_ns`; 2 × 1024 × 4 B namespace heads; NSRG JSON section. |

**Policy:** the current decoder must always be able to restore the two most recent
snapshot format versions. V5 support is maintained until a V7 is released.

---

## Event log format compatibility

| Version | Status | Notes |
|---|---|---|
| `v2` | Legacy | No cross-segment chain continuity header. |
| `v3` | Current | Segment header records the closing chain head of the previous segment. Cross-segment splice is verified during recovery. |

**Policy:** the replay path in `valori-storage` must handle both `v2` and `v3` segments
in the same log directory (mixed-version recovery is valid after a rolling upgrade).

---

## PlannerFingerprint compatibility

A cached `ExecutionGraph` is reusable when the full triple matches:
`(OperationHash, PlannerFingerprint.hash, PlanningContextHash)`.

**Breaking changes that require a new fingerprint:**

- Changes to routing logic (which task handles which operation kind).
- Changes to feature flags that affect graph structure.
- Changes to `metadata_schema_version` (new Collection or Shard fields).

**Non-breaking changes (fingerprint unchanged):**

- Performance improvements that don't alter the task graph.
- Adding new operation kinds (existing graphs are unaffected).
- Logging and observability changes.

---

## Wire type (valori-wire) compatibility

`valori-wire` crate follows semver strictly:

- **Patch**: No wire format changes.
- **Minor**: Additive changes only — new optional fields with serde defaults, new enum variants at the end.
- **Major**: Any breaking change — field removal, type change, required field added, enum variant reordering.

The Python SDK and CLI pin to a minor-compatible range. A major version bump
requires coordinated SDK release.

---

## HTTP API compatibility

URL path prefixes are versioned:

- `/v1/` — stable; breaking changes require a new prefix `/v2/`.
- `/v1/cluster/*` — cluster management plane; stable.
- Endpoints without a version prefix — internal or deprecated; no stability guarantee.

**Breaking change** = removing a required request field, removing a response field
that callers depend on, or changing the semantics of an existing field.

**Non-breaking** = adding optional request fields, adding response fields.

---

## Cluster rolling upgrade policy

A Valori cluster may run at most **two consecutive minor versions** simultaneously
during a rolling upgrade. Example: `v0.2.x` and `v0.3.x` nodes may coexist.
`v0.2.x` and `v0.4.x` may not.

Requirements for a version to support rolling upgrade with its predecessor:
1. `KernelABI` unchanged, OR the new version decodes the old `KernelEvent` format.
2. `ClientRequest` struct in `valori-wire` is backward-compatible (additive only).
3. The old version does not reject Raft log entries from the new version.

---

## Migration history

| Date | From | To | What changed | Migration required |
|---|---|---|---|---|
| 2025-Q2 | Snapshot V5 | Snapshot V6 | Added namespace metadata (per-record `namespace_id`, heads array, NSRG section) | V5 snapshots restored with all records in `DEFAULT_NS`; no data loss but namespace assignments reset. |

---

*Update this document whenever a versioned boundary changes. A PR that bumps
`KERNEL_ABI_VERSION`, the snapshot version constant, or the event log format
without updating this file will be rejected.*
