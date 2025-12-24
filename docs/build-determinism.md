# Build Determinism and Toolchain Contract

Valori is a deterministic memory kernel.  
As such, **the compiler is considered part of the execution environment**.

## Why Stable Rust (and not Nightly)?

Valori intentionally builds using a pinned **stable Rust toolchain** (`rust:1.85-slim` or newer).

We do **not** use Rust nightly for the following reasons:

- Nightly toolchains change daily and are not reproducible long-term
- Compiler optimizations and codegen may change between nightlies
- Deterministic memory systems require **bit-stable binaries**
- Auditable and replayable systems must be rebuildable years later

Nightly is excellent for experimentation.  
It is not appropriate for deterministic state machines.

## Why `-slim` Images?

The `-slim` base image is used to:

- Minimize the runtime attack surface
- Reduce non-essential system dependencies
- Improve auditability and supply-chain clarity

The Valori Node binary is built in a separate builder stage and copied into a minimal runtime image.
No runtime compilation occurs.

## Toolchain Requirement

Valori currently requires:

- **Rust ≥ 1.85 (stable)**
- Locked dependency resolution (`Cargo.lock` enforced)

If the toolchain or lockfile changes, the build is considered invalid.

This is a design choice, not a limitation.
