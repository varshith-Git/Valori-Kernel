# Migration (snapshot / wire-format versioning)

## Scope note — read this first
This repo has **no SQL migrations** — no `migrations/` directory, no Postgres/RLS to migrate (that's the
separate `valori-ui` repo). "Migration" here means **snapshot and wire-format version compatibility**:
`valori-kernel`'s persisted snapshot format and `valori-wire`'s event-log format. Verified current version:
```
crates/valori-kernel/src/snapshot/encode.rs:22
pub const SCHEMA_VERSION: u32 = 8;   // V8: adds KernelState.namespace_configs (per-collection dim/metric/index_kind)
```
**Both `CLAUDE.md` and `AGENTS.md` currently say V6 is current — that's stale relative to this constant.**
Always trust `SCHEMA_VERSION` in code over any doc's version table, including this repo's own root docs,
until they're corrected.

**Source of truth:** [`docs/SNAPSHOT_FORMAT.md`](../../docs/SNAPSHOT_FORMAT.md) for the wire format itself;
[`INVARIANTS.md`](../../INVARIANTS.md) I-12 (every persisted/networked type needs an explicit version) and
I-15 (`KernelABI` changes only on wire-format changes, not internal refactors) for the rules governing
when a version bump is required at all.

## When to use
A change to `crates/valori-kernel/src/snapshot/{encode,decode}.rs`, `crates/valori-wire`'s event-log
format, or any struct that's serialized to disk / sent over the network / included in a `Receipt`.

## Steps

1. **Does this actually need a version bump?** Per I-15: only if the binary wire format changes in a way
   that breaks backward compatibility. A pure internal refactor, performance improvement, or new in-memory
   structure that doesn't change serialization does **not** bump `SCHEMA_VERSION`/`KERNEL_ABI_VERSION`.
   Don't bump reflexively.
2. If it does: increment `SCHEMA_VERSION` in `encode.rs`, add the encode-side change, add the
   corresponding decode-side handling for **both** the new version and every still-supported old version
   (the current decoder is explicitly backward-compatible — e.g. V5 snapshots restore into an empty
   namespace registry rather than failing).
3. Update `docs/SNAPSHOT_FORMAT.md`'s versioning section and the version-history table in `CLAUDE.md`/
   `AGENTS.md` (see `.claude/rules/testing.md` — doc updates aren't optional when the format itself changes).
4. Add/update tests in `crates/valori-kernel/tests/format.rs` — round-trip (encode → decode → same state)
   for the new version, and specifically a test that an *old*-version snapshot still decodes correctly
   under the new decoder.
5. Consider the two-way compatibility case explicitly, same spirit as an app/schema migration: an **old
   node reading a new-format snapshot** (should fail cleanly, not corrupt state) and a **new node reading
   an old-format snapshot** (should succeed via the backward-compat path). State which of these two you
   verified.

## Required verification
```
cargo test -p valori-kernel                          # includes tests/format.rs
cargo miri test -p valori-kernel --test proof         # if hashing/receipt shape changed
```
Manually confirm: does `docs/SNAPSHOT_FORMAT.md`'s "Versioning" section still match `SCHEMA_VERSION`?

## Prefer additive
Adding a new section/field with a documented default for absent-in-old-format data (the namespace-heads-
array pattern used for V6, the `namespace_configs` addition in V8) over changing the meaning of an
existing field in place. A field whose meaning changed silently between two snapshot versions is exactly
what I-12 (explicit versioning) exists to prevent.

## Never do
- Never bump `SCHEMA_VERSION` for a change that doesn't touch the actual encoded bytes (I-15).
- Never remove backward-compatible decode support for an old version without an explicit decision that
  those old snapshots are no longer expected to exist anywhere — this is a durability decision, not a
  code-cleanup one.
- Never let a version bump land without a `tests/format.rs` case proving the old-format decode path still
  works.
