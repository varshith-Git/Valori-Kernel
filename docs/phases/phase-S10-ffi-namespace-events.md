# Phase S10 — Fix `valoricore-ffi` compile break (missing namespace-event match arms)

Branch: `Node-scaleup` (S1-S9 merged, `eaa9875`).

## Goal

`valoricore-ffi` (the embedded/local Python FFI crate, built via `maturin`
for `pip install -e python/`) did not compile: `get_timeline()`'s
exhaustive `match` over `KernelEvent` had no arms for
`AutoCreateNamespace`/`DropNamespace`, added back in S2. Confirmed this
predates the S1-S9 sharding work entirely — reproduced the identical
`E0004` at S4's commit (`809b87a`) and on `main` itself before touching
anything in this phase. Flagged as a known, deliberately out-of-scope gap
in the S1-S9 memory/follow-ups; this phase closes it since it was cheap and
the user asked for it directly.

## Delivered

Added the two missing arms to `get_timeline()`'s event-to-string match in
`crates/valori-ffi/src/lib.rs`, following the existing pattern for every
other `KernelEvent` variant (human-readable one-line summary per event, no
behavior change — `get_timeline()` is a read-only debug/audit helper):

```rust
KernelEvent::AutoCreateNamespace { name } =>
    format!("Event ID {event_id}: AutoCreateNamespace (Name: {name:?})"),
KernelEvent::DropNamespace { name } =>
    format!("Event ID {event_id}: DropNamespace (Name: {name:?})"),
```

## Findings

- `cargo build -p valoricore-ffi` (plain, no `maturin`) fails to *link*
  even after this fix — with pyo3 `extension-module`-feature-specific
  linker errors (`_Py_GetVersion`, `_Py_IncRef`, etc. "symbol(s) not
  found"). Confirmed this is **not** a regression: reproduced the
  identical linker failure on `main` (`a92a749`) with zero changes
  applied. This is expected, by-design PyO3 behavior — the
  `extension-module` feature (set in `crates/valori-ffi/Cargo.toml`)
  deliberately omits linking `libpython`, and the crate can only be built
  correctly via `maturin` (or `pip install -e python/`, which invokes
  maturin under the hood) — never via a bare `cargo build`. This project's
  own documented command for this crate is `pip install -e python/`
  (`CLAUDE.md`'s Commands section), never `cargo build -p valoricore-ffi`.
- Verified the actual fix (not just "no longer the wrong error") by
  building the real artifact: `maturin build --release` from
  `crates/valori-ffi/` succeeds and produces
  `target/wheels/valoricore_ffi-0.2.4-cp39-abi3-macosx_11_0_arm64.whl`.

## Validation

```
maturin build --release        # succeeds, wheel produced (crates/valori-ffi/)
cargo build --workspace --exclude valoricore-ffi   # clean, unaffected
```

`cargo test -p valori-kernel -p valori-node -p valori-consensus -p valori-cli`
was not re-run for this phase — the change is confined to one file in a
crate none of those test suites touch, and the fix's own correctness is
verified by the artifact build above (an exhaustive match either compiles
completely or not at all; there is no partial-success state to regression-test
here).

## Follow-ups

None — this closes the last item flagged in the S1-S9 sharding-initiative
memory. `cargo build --workspace` (without `--exclude valoricore-ffi`) will
still fail if invoked directly, by design (see Findings) — use
`cargo build --workspace --exclude valoricore-ffi` for a plain Cargo build,
or `maturin build`/`pip install -e python/` when the FFI wheel itself needs
building.
