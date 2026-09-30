# Rust rules

Verified against the workspace `Cargo.toml`, `Cargo.lock`, and `ci.yml`. This is not a Rust style guide —
only Valori-specific policy that isn't obvious from `rustfmt`/`clippy` defaults.

## Formatting & linting (CI-enforced — `ci.yml`'s `fmt`/`clippy` jobs)

```
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
```
Run both before calling anything done. `Cargo.toml`'s `[workspace.lints.rust]` / `[workspace.lints.clippy]`
already silence a long, deliberate list of noisy lints (e.g. `unused_imports`, `type_complexity`,
`too_many_arguments`) — don't re-litigate those in review; do treat everything still enabled as real.

## Panics and `unwrap`/`expect`

Both dev and release profiles set `panic = "abort"` — the comment in `Cargo.toml` is explicit: *"Firmware
requires abort on panic (no unwinding support in no_std). This also guarantees fail-closed behavior for
the Node."* A panic is a hard process stop, not a recoverable exception, everywhere in this workspace.
Consequences:
- `valori-node/src/main.rs` documents `try_recover()` as "never panics; on failure it logs" — that's the
  pattern for startup/recovery paths: log and degrade, don't panic.
- In `valori-kernel`: no panics in `apply_event_ns()` or any path that runs during replay — a panic mid-replay
  is a denial-of-service on every future restart from that log. Return `Result` and let the caller decide.
- `unwrap()`/`expect()` are fine in tests and in code that's provably infallible (e.g. indexing a
  const-generic array bound already checked) — not as a substitute for real error handling on any I/O,
  parsing, or network path.

## `unsafe`

11 files currently use it, concentrated in exactly the places you'd expect: fixed-point math hot paths
(`valori-kernel/src/math/{l2,dot}.rs`), index structures (`valori-index/src/{hnsw,ivf}.rs`), storage
(`valori-storage/src/object_store.rs`, `.../events/event_commit.rs`), a path utility
(`valori-studio-storage/src/path.rs`), and CLI benchmarks. New `unsafe` outside a genuine hot-path
performance reason needs a `// SAFETY:` comment explaining the invariant that makes it sound — not
"faster," a specific reason the safe alternative doesn't work. If you're adding `unsafe` to code that
isn't already on one of these hot paths, that's a signal to look for the safe version first.

## `no_std` / `wasm` boundary

`valori-kernel` is `no_std` (see `.claude/rules/determinism.md` and `AGENTS.md` invariant 7) — this is the
one crate where accidentally pulling in `std`, filesystem access, threads, or networking via a transitive
dependency is a real, easy-to-miss mistake. Before adding a dependency to `crates/valori-kernel/Cargo.toml`,
check it builds with `default-features = false` and doesn't drag in `std`. Verify with:
```
cargo build -p valori-kernel --target wasm32-unknown-unknown
```
`valori-node`, `valori-consensus`, `valori-cli` opt into `std` explicitly via `features = ["std"]` —
that's intentional, don't "fix" it.

## Workspace dependency policy

- Shared package metadata (`version`, `edition`, `license`, `authors`, `repository`) is set once in
  `[workspace.package]` and inherited via `.workspace = true` — don't hardcode a crate's own version/edition.
- `cargo deny check` runs on every PR (`.github/workflows/cargo-deny.yml`, config in `deny.toml`) — license
  and advisory-checked. A new dependency with an unapproved license or a known advisory fails CI, not a
  human review. Run it locally before adding a new dependency: `cargo deny check`.
- `crates/valori-embedded` is intentionally excluded from the workspace (path dep on a sibling repo not
  checked in) — don't add it back to `members`.

## Async runtime, logging, serialization

- `valori-node` is the async/tokio boundary; `valori-kernel` has none (`no_std`, synchronous state
  machine). Don't introduce `async fn` inside `valori-kernel`.
- Wire/persisted types go through `valori-wire` (serde structs, versioned V2/V3/V4 event-log format) —
  see `INVARIANTS.md` I-12: anything serialized to disk or sent over the network needs an explicit version
  field or a versioned envelope, not implicit Rust-type evolution.

## Tests for semantic behavior changes

Any change to a public function's *behavior* (not just its signature) needs a test that would fail
without the change — see `.claude/rules/testing.md` for which test tier a given change requires.
