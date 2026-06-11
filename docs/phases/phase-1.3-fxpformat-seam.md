# Phase 1.3 — FxpFormat seam: configurable precision

**Status:** done · on `multinode`
**Roadmap:** [MULTINODE_ROADMAP.md](../MULTINODE_ROADMAP.md) § 1.3

## Goal

Make arithmetic precision a first-class, identity-defining parameter:
trait contract for formats, format ID in every persistence header and in
the state-hash domain — so activating Q8.8/Q32.32 later is additive work,
not a migration. Deliberately **not** delivered: the viral
`KernelState<F>` generics — the engine stays hardcoded to Q16.16 until a
real customer demands a second format (the roadmap's own discipline:
build the seam, not the feature).

## Delivered

**`crates/valori-kernel/src/fxp/format.rs`** — the `FxpFormat` trait:

| Format | `Repr` | `Wide` (accumulator) | `FRAC_BITS` | `FORMAT_ID` | Status |
|---|---|---|---|---|---|
| `Q16_16` | i32 | i64 | 16 | 1 | implemented (production) |
| `Q8_8` | i16 | i32 | 8 | 2 | reserved, refused everywhere |
| `Q32_32` | i64 | i128 | 32 | 3 | reserved, refused everywhere |

`Wide` is part of the contract because dot products overflow `Repr` —
the detail that bites every retrofit. Const-asserts tie
`Q16_16::FRAC_BITS` to the legacy `config::FRAC_BITS`/`SCALE` constants
so the trait and the existing arithmetic can never drift.
`ACTIVE_FORMAT_ID` is the single constant everything stamps;
`parse_format`/`format_name` translate config strings.

**Hash-domain separation** (`snapshot/blake3.rs`) — the state hash now
opens with `"valori-state" || STATE_HASH_DOMAIN_VERSION (=2) ||
ACTIVE_FORMAT_ID`. A Q8.8 state can never hash-collide with a Q16.16
state, and future input-schema changes bump the domain version — hash
changes become versioned, visible events instead of silent drift.

**Snapshot V5** — header gains the format byte after the capacity block;
decode accepts V1–V5 (pre-V5 implies Q16.16) and **refuses** a snapshot
whose format byte doesn't match the active format, since restoring it
would silently corrupt every distance computation.

**`VALORI_FORMAT`** (node config) — defaults to `q16.16`. Unlike every
other config knob this never falls back silently: a recognized-but-
unimplemented format ("q8.8") and an unknown string both stop the
process with a message naming the known and implemented sets. Precision
is the one setting where "default away the typo" would be data
corruption.

(The wire-format slot was already delivered in Phase 1.2: v3 log headers
carry `format_id`, and `parse_header` refuses unknown IDs.)

## Findings

No new bugs — but one accounting note: ⚠️ **state-hash break #2** (after
1.1b's tag/metadata fix). The domain prefix changes every hash. This is
the *last* unversioned break: from now on the pinned-golden-hash test
fails CI on any accidental change, and deliberate changes bump
`STATE_HASH_DOMAIN_VERSION` in the same commit that updates the pin.

## Validation

- Full suite: **162 tests passing, 0 failures**
- New `tests/format.rs`: ID stability, accumulator widths, config
  parsing, snapshot V5 round-trip, foreign-format snapshot refused at
  the exact header byte, and the **pinned empty-state golden hash**
  (`4eeaa41d…4d4a`) that locks the hash domain in CI

## Follow-ups

- Full kernel generics over `FxpFormat` — only when a second format has
  a customer (Phase 4 in the roadmap)
- FFI/Python `format=` parameter — lands with the next SDK release
  cycle; the server-side validation is already in place
