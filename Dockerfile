# Multi-stage distroless build — Phase 1.11
# Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
#
# Stage 1: build  — full Rust toolchain; produces a statically-linked binary.
# Stage 2: runtime — Google distroless (no shell, no package manager, minimal CVE surface).
#
# Build:  docker build -t valori-node:latest .
# Run:    docker run -p 3000:3000 -v $(pwd)/data:/data valori-node:latest
# Health: docker inspect --format '{{.State.Health.Status}}' <container>

# ── Stage 1: build ─────────────────────────────────────────────────────────────
FROM rust:1.82-slim-bookworm AS builder

WORKDIR /build

# System deps for openssl / linking.
RUN apt-get update && \
    apt-get install -y --no-install-recommends \
        pkg-config libssl-dev ca-certificates musl-tools && \
    rm -rf /var/lib/apt/lists/*

# Cache dependencies before copying source.
# Copy only Cargo manifests first; the build layer is invalidated only when
# dependencies change, not on every source edit.
COPY Cargo.toml Cargo.lock ./
COPY crates/valori-kernel/Cargo.toml    crates/valori-kernel/
COPY crates/valori-node/Cargo.toml      crates/valori-node/
COPY crates/valori-wire/Cargo.toml      crates/valori-wire/
COPY crates/valori-cli/Cargo.toml       crates/valori-cli/
COPY crates/valori-verify/Cargo.toml    crates/valori-verify/
COPY crates/valori-consensus/Cargo.toml crates/valori-consensus/
# valori-ffi and embedded are non-default workspace members but must be present
# for workspace resolution even though valori-node never depends on them.
COPY crates/valori-ffi/Cargo.toml       crates/valori-ffi/
COPY embedded/Cargo.toml                embedded/

# Stub src files to allow `cargo build` to populate the dep cache.
# valori-ffi / embedded stubs are inert (not in valori-node's dep graph).
RUN for crate in valori-kernel valori-node valori-wire valori-cli valori-verify valori-consensus valori-ffi; do \
        mkdir -p crates/$crate/src && \
        printf 'pub fn stub() {}\n' > crates/$crate/src/lib.rs && \
        printf 'fn main() {}\n' > crates/$crate/src/main.rs; \
    done && \
    mkdir -p embedded/src && printf 'fn main() {}\n' > embedded/src/main.rs

RUN cargo build --release -p valori-node --locked 2>/dev/null || true

# Now copy the real source and build.
COPY . .

RUN touch crates/*/src/*.rs && \
    cargo build --release -p valori-node --locked

# ── Stage 2: runtime (distroless) ─────────────────────────────────────────────
# gcr.io/distroless/cc-debian12 provides the C runtime (libc, libgcc) needed
# by Rust binaries compiled against the system glibc. No shell, no apt, no
# package manager — attack surface is minimal.
FROM gcr.io/distroless/cc-debian12:nonroot

# Data directory — mount a volume here for persistent storage.
VOLUME ["/data"]

# Copy the compiled binary.
COPY --from=builder /build/target/release/valori-node /usr/local/bin/valori-node

# Environment defaults — override at runtime via -e or compose env:.
ENV VALORI_BIND=0.0.0.0:3000
ENV VALORI_DATA_DIR=/data
ENV VALORI_EVENT_LOG_PATH=/data/events.log
ENV VALORI_SNAPSHOT_PATH=/data/state.snap

EXPOSE 3000
EXPOSE 3100

# Distroless has no curl; the binary probes its own TCP port and exits 0/1.
# start_period=20s gives Raft time to elect a leader before retries count.
HEALTHCHECK --interval=15s --timeout=3s --start-period=20s --retries=3 \
    CMD ["/usr/local/bin/valori-node", "--health-check"]

USER nonroot
ENTRYPOINT ["/usr/local/bin/valori-node"]
