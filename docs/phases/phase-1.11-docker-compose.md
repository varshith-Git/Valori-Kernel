# Phase 1.11 — Docker + Compose: Multi-Stage Distroless + 3-Node Local Rig

**Status:** planned  
**Roadmap:** [MULTINODE_ROADMAP.md](../MULTINODE_ROADMAP.md) § 1.11  
**Why now:** The current `Dockerfile` is a single-stage `rust:slim` →
`debian:bookworm-slim` build that ships the full Rust toolchain in the
intermediate layer and copies only the binary. It works, but: (a) the image
is larger than necessary, (b) there is no `docker-compose.yml` at all, and
(c) Phase 2 needs a local 3-node topology from day one of development. The
compose file *is* the Phase 2 dev rig — getting it right in Phase 1 means
Phase 2 can focus on Raft, not on Docker YAML.

---

## Goal

1. **Multi-stage distroless Dockerfile** — builder stage compiles `valori-node`
   with `--locked`; runtime stage uses `gcr.io/distroless/cc-debian12` (no
   shell, no package manager, minimal attack surface, ~5 MB runtime layer).
2. **`docker-compose.yml`** — 3-node topology for local development; single-node
   useful today; becomes Phase 2's Raft dev rig by adding `--features cluster`
   and peer discovery configuration. Nodes share a bridge network; each has
   its own named volume.
3. **Health-check plumbing** — each container uses `GET /v1/health` as the
   Docker health check; compose service depends_on is expressed as
   `condition: service_healthy` so startup ordering is automatic.
4. **`Makefile` targets** — `make docker-build`, `make compose-up`,
   `make compose-hash-check` (verifies all 3 nodes report the same state hash
   after a smoke write) so the topology is runnable in one command.

---

## D1 — Multi-Stage Distroless Dockerfile

```dockerfile
# syntax=docker/dockerfile:1.7
# Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.

# ── Stage 1: Build ────────────────────────────────────────────────────────────
FROM rust:1.82-slim-bookworm AS builder
WORKDIR /build

# Dependency layer (cached separately from source for faster rebuilds)
COPY Cargo.toml Cargo.lock ./
COPY crates/valori-kernel/Cargo.toml  crates/valori-kernel/Cargo.toml
COPY crates/valori-wire/Cargo.toml    crates/valori-wire/Cargo.toml
COPY crates/valori-node/Cargo.toml    crates/valori-node/Cargo.toml
COPY crates/valori-verify/Cargo.toml  crates/valori-verify/Cargo.toml
COPY crates/valori-cli/Cargo.toml     crates/valori-cli/Cargo.toml
COPY crates/valori-consensus/Cargo.toml crates/valori-consensus/Cargo.toml

# Stub sources so cargo fetch can resolve the workspace
RUN find crates -name Cargo.toml -exec sh -c \
    'mkdir -p $(dirname {}/src); echo "fn main(){}" > $(dirname {})/src/main.rs' \; 2>/dev/null || true
RUN cargo fetch --locked

# Full source
COPY . .

# Build release binary. --locked ensures Cargo.lock is respected.
RUN cargo build --release --locked \
    -p valori-node \
    --no-default-features

# Strip the binary to reduce size (~40% reduction typical)
RUN strip target/release/valori-node

# ── Stage 2: Runtime ──────────────────────────────────────────────────────────
FROM gcr.io/distroless/cc-debian12:nonroot AS runtime
# nonroot: runs as uid 65532 (nobody) — no root escalation possible

LABEL org.opencontainers.image.title="valori-node"
LABEL org.opencontainers.image.description="Deterministic vector + knowledge graph database node"
LABEL org.opencontainers.image.source="https://github.com/varshith-gudur/Valori-Kernel"
LABEL org.opencontainers.image.licenses="MIT OR Apache-2.0"

# Copy binary only — no shell, no apt, no package manager in the final image
COPY --from=builder /build/target/release/valori-node /usr/local/bin/valori-node

# Data directory — must be a named volume in production
VOLUME ["/data"]

# Config via environment variables (see docs/DEPLOYMENT.md)
ENV VALORI_BIND=0.0.0.0:3000 \
    VALORI_EVENT_LOG_PATH=/data/audit/events.log \
    VALORI_SNAPSHOT_PATH=/data/snapshots/state.snap \
    VALORI_MAX_RECORDS=65536 \
    VALORI_DIM=384 \
    VALORI_MAX_NODES=65536 \
    VALORI_MAX_EDGES=131072

EXPOSE 3000

# Health check: GET /v1/health returns 200 when the node is ready
# --interval 5s  → check every 5 seconds
# --timeout 3s   → fail if no response in 3 seconds
# --retries 3    → mark unhealthy after 3 consecutive failures
# --start-period 10s → give the node time to recover from snapshot on boot
HEALTHCHECK --interval=5s --timeout=3s --retries=3 --start-period=10s \
    CMD ["/usr/local/bin/valori-node", "--health-check"]
# NOTE: "--health-check" is a new flag (Phase 1.11) that calls GET /v1/health
#       internally and exits 0 (healthy) or 1 (unhealthy/starting).
#       Implementation: a small tokio::main binary using reqwest to
#       http://localhost:$PORT/v1/health.
#       Alternative: ship a static `curl`-equivalent; distroless has no curl.
#       Chosen approach: self-health-check sub-command (no extra binary).

ENTRYPOINT ["/usr/local/bin/valori-node"]
```

### Image size target

| Layer | Expected size |
|---|---|
| `distroless/cc-debian12:nonroot` base | ~15 MB |
| `valori-node` stripped binary | ~12 MB |
| **Total** | **~27 MB** |

The current `debian:bookworm-slim` approach produces ~90 MB. Distroless reduces
the attack surface to glibc + the binary; no shell means no exec-based escapes.

---

## D2 — `docker-compose.yml` (3-Node Local Rig)

```yaml
# docker-compose.yml  [REPLACE — Phase 1.11]
# Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
#
# Three-node Valori topology for local development.
#
# Today (Phase 1): all three nodes run in standalone mode (no Raft).
#   They share a bridge network but don't replicate — useful for testing
#   independent persistence, health checks, and load-balancer routing.
#
# Phase 2 (cluster mode): set VALORI_MODE=cluster and add --features cluster
#   to the Dockerfile build. The --peers ENV wires up Raft peer discovery.
#   The compose file structure is identical; only ENV vars change.
#
# Usage:
#   make compose-up          # start 3 nodes
#   make compose-hash-check  # verify all nodes have same state hash
#   docker compose down -v   # stop + delete volumes

name: valori-dev

networks:
  valori-net:
    driver: bridge
    ipam:
      config:
        - subnet: 172.20.0.0/24

volumes:
  node1-data:
  node2-data:
  node3-data:

x-valori-common: &valori-common
  image: valori-node:dev
  build:
    context: .
    dockerfile: Dockerfile
    target: runtime
  restart: on-failure:3
  networks:
    - valori-net
  environment:
    VALORI_MAX_RECORDS: "65536"
    VALORI_DIM: "384"
    VALORI_MAX_NODES: "65536"
    VALORI_MAX_EDGES: "131072"
    VALORI_FORMAT: "q16.16"
    # Phase 1: standalone mode (no Raft)
    # Phase 2: set VALORI_MODE=cluster and VALORI_NODE_ID / VALORI_PEERS

services:
  node1:
    <<: *valori-common
    hostname: node1
    container_name: valori-node1
    ports:
      - "3001:3000"
    volumes:
      - node1-data:/data
    environment:
      VALORI_BIND: "0.0.0.0:3000"
      VALORI_NODE_ID: "1"                       # reserved for Phase 2
      VALORI_EVENT_LOG_PATH: /data/audit/events.log
      VALORI_SNAPSHOT_PATH: /data/snapshots/state.snap
      # Phase 2: VALORI_PEERS: "node2:3100,node3:3100"
    healthcheck:
      test: ["/usr/local/bin/valori-node", "--health-check"]
      interval: 5s
      timeout: 3s
      retries: 3
      start_period: 10s

  node2:
    <<: *valori-common
    hostname: node2
    container_name: valori-node2
    ports:
      - "3002:3000"
    volumes:
      - node2-data:/data
    environment:
      VALORI_BIND: "0.0.0.0:3000"
      VALORI_NODE_ID: "2"
      VALORI_EVENT_LOG_PATH: /data/audit/events.log
      VALORI_SNAPSHOT_PATH: /data/snapshots/state.snap
      # Phase 2: VALORI_PEERS: "node1:3100,node3:3100"
    depends_on:
      node1:
        condition: service_healthy
    healthcheck:
      test: ["/usr/local/bin/valori-node", "--health-check"]
      interval: 5s
      timeout: 3s
      retries: 3
      start_period: 10s

  node3:
    <<: *valori-common
    hostname: node3
    container_name: valori-node3
    ports:
      - "3003:3000"
    volumes:
      - node3-data:/data
    environment:
      VALORI_BIND: "0.0.0.0:3000"
      VALORI_NODE_ID: "3"
      VALORI_EVENT_LOG_PATH: /data/audit/events.log
      VALORI_SNAPSHOT_PATH: /data/snapshots/state.snap
      # Phase 2: VALORI_PEERS: "node1:3100,node2:3100"
    depends_on:
      node2:
        condition: service_healthy
    healthcheck:
      test: ["/usr/local/bin/valori-node", "--health-check"]
      interval: 5s
      timeout: 3s
      retries: 3
      start_period: 10s
```

### `depends_on` chain rationale

In standalone mode the nodes don't communicate, so `node1 → node2 → node3`
is a documentation choice, not a functional requirement. In Phase 2, the
leader (node1) must be healthy before followers attempt to join the Raft group.
The chain already encodes the correct order.

---

## D3 — `--health-check` Sub-Command

Distroless has no `curl` or `wget`. The health check probe is a sub-command
in the same `valori-node` binary:

```rust
// crates/valori-node/src/main.rs  [MODIFY — Phase 1.11]

// args parsing:
if args.health_check {
    // Determine port from VALORI_BIND env (default 3000)
    let port = std::env::var("VALORI_BIND")
        .unwrap_or_else(|_| "0.0.0.0:3000".into())
        .split(':').last().unwrap_or("3000")
        .parse::<u16>().unwrap_or(3000);

    let url = format!("http://127.0.0.1:{port}/v1/health");

    // Blocking reqwest (feature-gated "blocking") — health check is a one-shot
    match reqwest::blocking::get(&url) {
        Ok(resp) if resp.status().is_success() => std::process::exit(0),
        Ok(resp) => {
            eprintln!("health check: {}", resp.status());
            std::process::exit(1);
        }
        Err(e) => {
            eprintln!("health check: {e}");
            std::process::exit(1);
        }
    }
}
```

`reqwest` is already a dependency (`Cargo.toml` line 20). `blocking` feature
is added to the feature list.

### CLI arg

```rust
// clap Args struct
/// Run a health check against the local server and exit 0 (healthy) or 1.
/// Used by Docker HEALTHCHECK.
#[arg(long, hide = true)]
health_check: bool,
```

`hide = true` — the flag is in `--help` but not prominently listed. It is an
implementation detail of the Docker health-check plumbing, not a user-facing
command.

---

## D4 — Makefile Targets

```makefile
# Additions to Makefile  [MODIFY — Phase 1.11]

# ── Docker ────────────────────────────────────────────────────────────────────

## Build the valori-node Docker image (tagged valori-node:dev)
docker-build:
	docker build -t valori-node:dev --target runtime .

## Start the 3-node local topology (detached)
compose-up: docker-build
	docker compose up -d
	@echo "Nodes starting... wait for health checks to pass (~15s)"
	@echo "  node1: http://localhost:3001/v1/health"
	@echo "  node2: http://localhost:3002/v1/health"
	@echo "  node3: http://localhost:3003/v1/health"

## Stop all nodes and remove data volumes
compose-down:
	docker compose down -v

## Write a test vector to node1, then verify all 3 nodes report the same
## state hash (standalone: should diverge; Phase 2 cluster: must converge).
compose-hash-check: compose-up
	@echo "Waiting for all nodes to be healthy..."
	@until curl -sf http://localhost:3001/v1/health > /dev/null; do sleep 1; done
	@until curl -sf http://localhost:3002/v1/health > /dev/null; do sleep 1; done
	@until curl -sf http://localhost:3003/v1/health > /dev/null; do sleep 1; done
	@echo ""
	@echo "State hashes (standalone: independent; Phase 2: should be equal):"
	@echo "  node1: $$(curl -sf http://localhost:3001/v1/proof/state | jq -r .state_hash)"
	@echo "  node2: $$(curl -sf http://localhost:3002/v1/proof/state | jq -r .state_hash)"
	@echo "  node3: $$(curl -sf http://localhost:3003/v1/proof/state | jq -r .state_hash)"

.PHONY: docker-build compose-up compose-down compose-hash-check
```

`compose-hash-check` is a human-readable debugging tool in Phase 1 (nodes
have independent state, so hashes differ). In Phase 2, the same target becomes
the cluster health verification step — if all three hashes match, the cluster
is consistent.

---

## D5 — `.dockerignore` Update

```dockerignore
# Additions to .dockerignore  [MODIFY — Phase 1.11]

# Exclude test data and Python artifacts
test_e2e_db/
valori_db/
valoricore_test_delete_db/
graphify-out/
notebooks/
*.log
*.py
*.sh

# Exclude CI and documentation
.github/
docs/
demo/
data/

# Exclude build artifacts (already excluded but explicit)
target/
target_tmp/
.npm-cache/

# Never exclude Cargo.lock (needed for --locked)
!Cargo.lock
```

The current `.dockerignore` is 5 lines. The additions reduce the build context
from ~200 MB to ~20 MB (mostly by excluding Python caches, test databases, and
build artifacts).

---

## Files Changed

| File | Action | Notes |
|---|---|---|
| `Dockerfile` | REPLACE | Multi-stage distroless; `--health-check` sub-command |
| `docker-compose.yml` | NEW | 3-node topology with health checks and named volumes |
| `.dockerignore` | MODIFY | Exclude Python, test data, docs |
| `Makefile` | MODIFY | `docker-build`, `compose-up`, `compose-down`, `compose-hash-check` |
| `crates/valori-node/src/main.rs` | MODIFY | `--health-check` arg + `reqwest::blocking` probe |
| `crates/valori-node/Cargo.toml` | MODIFY | Add `blocking` feature to `reqwest` |

---

## Phase 2 Upgrade Path

The compose file is explicitly designed to become the Phase 2 Raft dev rig
with minimal changes:

```yaml
# Phase 2 changes (3 ENV lines per node, no structural changes):
environment:
  VALORI_MODE: "cluster"           # enables Raft path
  VALORI_PEERS: "node2:3100,node3:3100"  # peer discovery
  VALORI_RAFT_PORT: "3100"         # Raft gRPC port (separate from HTTP)
```

Plus a `ports` addition for the Raft port (`3100`). The health check, volumes,
network, and `depends_on` chain stay unchanged.

---

## Acceptance Criteria

| Criterion | How verified |
|---|---|
| `docker build -t valori-node:dev .` succeeds | CI build job |
| Final image size < 35 MB | `docker image inspect valori-node:dev` |
| Image runs as non-root (uid 65532) | `docker run --rm valori-node:dev id` |
| `docker compose up -d` starts 3 healthy nodes | `docker compose ps` all show `healthy` |
| `GET /v1/health` returns 200 from all 3 nodes | `make compose-hash-check` |
| `--health-check` exits 0 when server is running | `docker exec valori-node1 valori-node --health-check` |
| `--health-check` exits 1 when server is not running | Tested in compose startup race |
| `.dockerignore` reduces build context to < 25 MB | `docker build --no-cache . 2>&1 | grep "Sending build context"` |

---

## Findings

Design-only phase — no runtime findings. One forward concern:

**`reqwest::blocking` in the binary:** Adding `blocking` feature to `reqwest`
pulls in an extra thread pool that is only used during the health-check probe
and is otherwise idle. For a long-lived server binary this is acceptable; for
an embedded binary it would not be. Mitigation: compile the `--health-check`
path behind `#[cfg(feature = "docker-health")]` if binary size becomes a concern.

**Distroless and crash debugging:** Distroless has no shell. If the server
crashes, `docker exec -it valori-node1 /bin/sh` fails. Mitigation: use
`docker logs valori-node1` (structured tracing output goes to stdout/stderr);
for core dump capture, use a sidecar debug container pattern or the
`:debug` variant of distroless (which includes busybox). Document in
`docs/DEPLOYMENT.md`.

## Follow-ups

- Phase 2: add `VALORI_MODE`, `VALORI_PEERS`, `VALORI_RAFT_PORT` to the
  compose template and document the 3-node cluster startup sequence.
- Phase 2: add a Prometheus + Grafana sidecar to the compose file
  (scrape `GET /metrics` from each node; the `state_hash_match` gauge is
  the signature dashboard panel).
- Phase 2: multi-platform image build (`linux/amd64` + `linux/arm64`) via
  `docker buildx` — this is the x86 + ARM mixed-arch demo mentioned in § 2.6.
- Phase 3: Helm chart (StatefulSet) derives from the same ENV contract as
  this compose file — the 3-node compose is the proof the ENV contract works.
