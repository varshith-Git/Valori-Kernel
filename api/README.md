# Valori API Contract

This directory holds the **canonical machine-readable contract** for Valori's
public API.

```
api/
└── openapi/
    └── valori-v1.yaml     ← the contract
```

---

## 1. What this is

`api/openapi/valori-v1.yaml` is the single source of truth for the **Valori
Data API v1** — the HTTP surface a `valori-node` process serves in either
standalone or cluster mode.

There is **one logical Valori API v1**. It may eventually have more than one
transport; today it has exactly one (REST/JSON), described by this OpenAPI
document.

## 2. Why it exists

Before this file, the API's definition was spread across four hand-maintained
copies that drifted from each other and from the implementation:

* the Rust request/response structs in `crates/valori-node/src/api.rs` and
  the two router files,
* the Python SDK's inline dictionaries in `python/valoricore/remote.py`,
* two duplicated hand-written TypeScript mirrors in `ui/src/types/valori.ts`
  and `ui/studio/src/types/valori.ts`,
* prose tables in `CLAUDE.md`, crate READMEs and phase reports.

Every one of those disagreed with the implementation in at least one place —
`docs/api/current-vs-target.md` catalogues 52 concrete divergences found during
the Phase API-1 audit. A contract that a machine can validate, generate from
and diff makes that class of drift a build failure rather than a support
ticket.

## 3. Scope — what is and is not in `valori-v1.yaml`

### In scope: the Data Plane

Collections, Records, Search, Multi-search, Indexes, Graph, GraphRAG, Memory,
Ingest, Tree-RAG, Community, Proof, Operations, Snapshots, Crypto, Cluster
status, API-key admin. Anything a client application calls to store, retrieve
or verify data.

### Out of scope, deliberately

| Excluded | Where it lives | Why |
|---|---|---|
| **Local control plane** — projects, workspaces, models, node start/stop | `crates/valori-daemon` (`/v1/projects/*`, `/v1/workspaces/*`, `/v1/models/*`) | Different process, different lifecycle, **no authentication** (loopback-only by design). Forcing project-lifecycle types into the data-plane contract would blur the boundary the architecture depends on. |
| **Valori Cloud** — organizations, regions, deployments, billing, provisioning, project API keys | Separate service (`/api/projects/{id}/…`, `/v1/regions`, `/v1/settings/public`, `/v1/projects/{id}/provision`) | Multi-tenant SaaS control plane. Different auth model, different resource graph, different release cadence. It will get its own contract. |
| **Embedded Python engine** | `python/valoricore/local.py` → `valoricore_ffi.abi3.so` → `crates/valori-ffi` → `valori-kernel` | In-process FFI. Never speaks HTTP. See §7. |
| **Deprecated legacy path aliases** — `/records`, `/search`, `/graph/*`, `/timeline`, `/operations`, `/version`, `/v1/vectors/batch_insert` | Still served, with `Deprecation: true` + `Link` headers | Documenting them in the contract would legitimise them and cause SDK generators to emit them. They are inventoried in `docs/api/api-inventory.md` and scheduled for removal in v2. |
| **Internal replication** — `/v1/replication/wal`, `/v1/replication/events`, `/v1/replication/state`, `/v1/cluster/read-index` | `valori-node` | Node-to-node protocol. `/v1/replication/events` is an unbounded live stream with no natural end — not a request/response shape. |
| **`GET /metrics`** | `valori-node` | A Prometheus scrape target, not a client API. |

### The Control Plane / Data Plane boundary

```
┌─────────────────────────┐      ┌──────────────────────────┐
│  Valori Cloud API       │      │  valori-daemon           │
│  (multi-tenant SaaS)    │      │  (one machine)           │
│  orgs, regions,         │      │  projects, workspaces,   │
│  deployments, billing,  │      │  models, node lifecycle  │
│  project API keys       │      │  NO AUTH — loopback only │
└───────────┬─────────────┘      └────────────┬─────────────┘
            │  provisions / proxies           │  spawns / supervises
            └───────────────┬─────────────────┘
                            ▼
                 ┌──────────────────────┐
                 │  valori-node         │  ◀── valori-v1.yaml
                 │  DATA PLANE          │      describes THIS
                 │  collections,        │      and only this
                 │  records, search,    │
                 │  graph, proofs       │
                 └──────────────────────┘
```

Both control planes **call** the data plane; neither is described by its
contract.

## 4. Ownership

| Concern | Owner |
|---|---|
| The contract file | Whoever changes the node's HTTP surface — the same PR |
| Reviewing contract changes | API review (breaking-change rules in §6) |
| Keeping it truthful | `docs/api/api-inventory.md` is the audit trail; regenerate it when routes change |

**Rule: a PR that changes a route, a request field, a response field or a
status code must change `valori-v1.yaml` in the same commit.** The contract
follows the code — it does not lead it, and it must never lag it.

## 5. Validation

```bash
# YAML syntax
python3 -c "import yaml; yaml.safe_load(open('api/openapi/valori-v1.yaml'))"

# Full OpenAPI validation (structure, $refs, examples against schemas)
npx --yes @redocly/cli@latest lint api/openapi/valori-v1.yaml
```

The document currently validates cleanly with four **intentional** warnings:

| Warning | Why it is intentional |
|---|---|
| `no-server-example.com` (×2) — servers point at `localhost` | Valori is self-hosted. There is no canonical public host to name; the third server entry is a templated `{scheme}://{host}:{port}`. |
| `operation-4xx-response` on `GET /health` | `/health` is unauthenticated and returns only 200 or 503. It has no 4xx by design. |
| `operation-4xx-response` on `GET /v1/version` | Resolved in Phase API-3.1 — the operation now documents its 401. |

### What the contract also asserts

* Every documented operation exists in code today — not as a claimed
  annotation but as a proven one: the document is generated from the registered
  handlers, so an operation cannot appear without a handler behind it.
  (`x-status` itself is not currently emitted; it has no Rust source of truth
  yet and inventing values would repeat the retracted Phase API-3. Tracked as a
  Phase API-4 follow-up.)
* Where one mode does not implement an operation, it carries
  `x-cluster-status` / `x-standalone-status: not_implemented`.
* Operator-only and admin endpoints are absent from the document entirely,
  rather than present-and-flagged. Every operation here carries `x-sdk: true`.
* Every operation records its minimum key scope in `x-required-scope`.

### Official API Contract Gate (Phase API-2.5)

Run the single official entry point to prove contract reproducibility, route parity, schema integrity, and client compatibility:

```bash
./scripts/api-contract-gate.sh
```

# Code-side conformance (Phase API-2)

Validation above proves the document is *well-formed*. These prove it matches
the *running node*:

```bash
# The Rust error taxonomy, the canonical DTOs, and standalone/cluster
# behaviour parity — 21 tests
cargo test -p valori-node --features utoipa --test api_contract

# The committed document IS the generator's output, byte for byte
cargo test -p valori-node --features utoipa --test openapi_generated

# Rust public routes == utoipa operations == OpenAPI operations
python3 scripts/verify-api-route-contract.py

# Both routers declare the same paths and methods
cargo test -p valori-node --test route_parity
```

## 5b. Generation status — the contract is generated, end to end

**Phase API-3.1.** `api/openapi/valori-v1.yaml` is no longer hand-maintained.
It is the byte-exact output of:

```bash
cargo run -p valori-node --features utoipa --bin valori-openapi -- \
    --output api/openapi/valori-v1.yaml
```

Never hand-edit it. `crates/valori-node/tests/openapi_generated.rs` fails if the
committed file differs from the generator's output by one byte, and prints the
first diverging line.

### The pipeline

```
Rust axum router registrations
      │
      ├─▶ scripts/generate-route-manifest.py   (discovery + classification only;
      │                                         never reads or writes OpenAPI)
      │
      └─▶ #[utoipa::path] on the registered handler
            + #[derive(ToSchema)] on the public DTO
            + registration in ValoriApi's paths(...)
                  │
                  ├─▶ VendorExtensionAddon   (x-required-scope, x-sdk — metadata
                  │                           only; cannot create an operation)
                  │
                  └─▶ valori-openapi ──▶ api/openapi/valori-v1.yaml
                                              │
                                              ├─▶ openapi-typescript ▶ ui/api-types
                                              └─▶ future SDKs
```

Verification is a separate, one-directional concern:
`scripts/verify-api-route-contract.py` reads all three sets and diffs them. It
never writes.

### Coverage

| | |
|---|---|
| Routes the node registers | 100 |
| Public SDK routes | **74** |
| utoipa operations | **74** |
| Operations in this file | **74** |
| Generated schemas | **143** |
| OpenAPI version | **3.1.0** |

Security metadata is generated, not written: every operation carries
`security` plus `x-required-scope`, the latter read out of
`crate::api_keys::required_scope` — the same function the auth middleware
calls — so the contract cannot claim a scope the server does not enforce. The
`401`/`403` responses come from `AuthResponsesAddon` for the same reason: they
are produced by the middleware, not the handlers, and both carry an empty body.
See `docs/api/security-contract.md`.

`metric` and `index` cross the wire as closed enums (`Metric`/`MetricInput`,
`IndexKind`/`IndexKindInput`), with the accepted set and the emitted set
modelled separately because `FromStr` takes aliases that `as_str` never
produces. `CreateCollectionRequest` requires `name`, `dimension`, and `metric`
— `"default"` included, no exception.

The other 26 routes — 7 admin, 5 operator-internal, 14 deprecated aliases — are
**served but deliberately excluded**. A server route is not a public SDK route.
If an operator contract is ever wanted it belongs in a separate document with
its own gate, not mixed into this one.

### Where the DTOs live

Public request/response types are deliberate API-boundary types. Most are in
`crates/valori-node/src/api.rs`. Where the wire model already belongs to another
crate — tree-RAG and community (`valori-rag`), ingest (`valori-ingest`), index
lifecycle (`valori-engine`), object store (`valori-storage`), model health
(`valori-models`) — that crate carries an optional, default-off `utoipa` feature
which `valori-node/utoipa` enables. The contract then references the same type
the handler serialises, so the schema cannot drift from the wire. `valori-kernel`
is excluded on purpose: it takes no dependency and stays `no_std`.

### Adding an endpoint

1. Register the route in **both** `server.rs` and `cluster_server.rs`
   (`tests/route_parity.rs` enforces this).
2. Annotate the registered handler with
   `#[cfg_attr(feature = "utoipa", utoipa::path(...))]`, giving it a real
   `operation_id`, request body, and per-status responses.
3. List the handler in `paths(...)` in `src/openapi.rs`, and its DTOs in
   `components(schemas(...))`. **Both** steps are required — an annotation that
   is not registered generates nothing.
4. Regenerate and run `./scripts/api-contract-gate.sh`.

### OpenAPI version

3.1.0. utoipa 5.5.0 cannot emit 3.0.x by construction, and the one generator
this repo runs (`openapi-typescript@7`) is 3.1-first. Full reasoning, the
alternatives considered, and the condition for revisiting:
`docs/api/openapi-version-decision.md`.

## 6. Versioning rules

The API version (`info.version`) is **not** the crate version. `valori-node`
can go from 0.2.4 to 0.9.0 without the API leaving v1.

### Breaking — requires `/v2/`

* Removing or renaming a path, an operation, a request field or a response field.
* Changing a field's type, or its units.
* Making an optional request field required.
* Changing the meaning of an existing value (e.g. redefining what `score` measures).
* Removing an enum value, or changing what an existing value means.
* Changing a success status code (200 → 202 and the like).
* Narrowing what an endpoint accepts.

### Non-breaking — ship in v1

* A new endpoint.
* A new **optional** request field with a behaviour-preserving default.
* A new response field (clients must ignore unknown fields).
* A new enum value **in a response**, when clients are documented to tolerate
  unknown values.
* Widening what an endpoint accepts.
* Documentation, examples, descriptions.
* Adding an error code to an already-declared status.

> Adding an enum value to a **request** enum is non-breaking for the server
> but changes generated client types; treat it as a minor version bump of the
> generated SDKs.

### Deprecation

1. Mark it `deprecated: true` in the contract and name the successor in the
   `description`.
2. Serve `Deprecation: true` and `Link: <successor>; rel="successor-version"`
   (RFC 8594) — the node already does this for legacy path aliases.
3. State the earliest version it may be removed in. Removal happens only at a
   major version.

Currently deprecated in v1: `GraphRagRequest.k` (use `retrieval_k`),
`GraphRagHit.score` (use `vector_score`), `namespace` on the tree/community/
entity endpoints (use `collection`), `GET /v1/index/config`.

## 7. Embedded engine vs remote SDK

Valori ships **two** Python paths, and only one of them is governed by this
contract.

```
REMOTE  (governed by valori-v1.yaml)
  python/valoricore/remote.py
      │  HTTP + JSON
      ▼
  valori-node  ──►  valori-engine ──► valori-kernel

EMBEDDED  (NOT governed by valori-v1.yaml)
  python/valoricore/local.py
      │  PyO3 FFI, in-process
      ▼
  valoricore_ffi.abi3.so  (crates/valori-ffi)
      │
      ▼
  valori-kernel
```

The embedded path has no HTTP layer, no auth, no cluster mode and no status
codes. `python/valoricore/factory.py` chooses between the two at runtime.
They are **not** proven feature-equivalent, and this contract must never be
read as implying they are. Any statement in `valori-v1.yaml` applies to the
remote path only.

## 8. Future — SDK generation (not implemented)

The intended architecture, for when SDK work begins:

```
api/openapi/valori-v1.yaml
        │
        ▼   OpenAPI Generator
  generated transport + models        (mechanical, regenerated, never edited)
        │
        ▼   thin hand-written layer
  ergonomic SDK surface               (naming, retries, leader-following,
        │                              pagination helpers, context managers)
        ▼
  Python · TypeScript · Go · Java · Rust
```

**Generated code is not the user-facing API.** It is an implementation detail
behind a hand-written ergonomic layer, so that idiomatic naming and
convenience behaviour survive regeneration.

Intended package names (**not yet reserved or published**):

| Language | Package |
|---|---|
| TypeScript | `@valori/sdk` |
| Python | `valori` |
| Go | `valori-go` |
| Java | `valori-java` |
| Rust | `valori-rs` |

The existing `valoricore` Python package predates this plan and would be
migrated, not replaced overnight.

## 9. Future — Protobuf / gRPC (not implemented)

**No `.proto` files exist and none are created by this contract.**

OpenAPI describes the **public REST contract** and will keep doing so.
Protobuf/gRPC is reserved for a **future internal service contract** —
strongly typed, streaming-capable communication between Valori's own
components:

```
Valori Control Plane  ◀── gRPC ──▶  Workers  ◀── gRPC ──▶  valori-node
```

When that lands it will live at `api/proto/`, describe the *same logical API
v1* for the data-plane parts it overlaps with, and be kept consistent with
this document by review, not by generation. It is explicitly out of scope for
Phase API-1.

## 10. Related documents

| Path | Contents |
|---|---|
| [`docs/api/api-inventory.md`](../docs/api/api-inventory.md) | Every externally reachable route, with method, domain, auth, scope, standalone/cluster support, request/response detail, status codes, idempotency, consistency and pagination behaviour |
| [`docs/api/current-vs-target.md`](../docs/api/current-vs-target.md) | 52-row gap analysis: what the code does vs what the contract says, with severity |
| [`docs/api/ui-parity.md`](../docs/api/ui-parity.md) | TypeScript / UI drift against the contract |
| [`COMPATIBILITY.md`](../COMPATIBILITY.md) | Repo-wide compatibility policy (kernel ABI, snapshot format, event log, wire types, HTTP API) |
| [`docs/architecture/control-plane.md`](../docs/architecture/control-plane.md) | Who owns what: daemon vs node vs UI |
| [`crates/valori-node/tests/route_parity.rs`](../crates/valori-node/tests/route_parity.rs) | Mechanically enforces standalone/cluster route-set parity |
