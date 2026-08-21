# Non-Public Route Boundary — Valori API v1

Generated from `docs/api/phase-api-3-route-manifest.json`, which
`scripts/generate-route-manifest.py` derives from the Rust router
registrations in `server.rs`, `cluster_server.rs`, and `cluster_api.rs`.
Every route below was read out of that manifest — none of it is hand-listed.

## Totals

| | Count |
|---|---|
| Routes registered in Rust | 100 |
| Public SDK contract (`PUBLIC_UNAUTH` + `PUBLIC_SDK`) | 74 |
| Non-public | 26 |

## Boundary invariant

Every registered route carries exactly one classification:

- `PUBLIC_UNAUTH` — public, no credentials consulted (`GET /health` only)
- `PUBLIC_SDK` — public, Bearer-authenticated, in the SDK contract
- `ADMIN` — operator control plane; key management and cluster topology
- `OPERATOR_INTERNAL` — node-to-node and scrape surfaces
- `DEPRECATED` — legacy unprefixed routes superseded by a `/v1` equivalent

There is no "unclassified" state. `scripts/verify-api-route-contract.py`
fails the gate if a non-public route appears in the public contract, and the
manifest generator fails if a route matches no classification rule.

## Why these are excluded

**`ADMIN`** mints and revokes credentials or reconfigures the deployment.
Shipping them in the public SDK would put cluster membership changes one
method call away from ordinary application code. They require
`x-required-scope: admin` at runtime today; see `docs/api/security-contract.md`.

**`OPERATOR_INTERNAL`** are not request/response product endpoints.
`GET /metrics` is a Prometheus scrape target, and `/v1/replication/*` are
node-to-node streams (`/events` is unbounded and has no natural end), so
neither fits a synchronous SDK method.

**`DEPRECATED`** are the pre-`/v1` spellings. Each has a `/v1` successor that
*is* in the public contract, so a generated SDK exposes the operation once,
under its supported path, rather than twice.

---

## ADMIN (7)

| Method | Path | Source |
|---|---|---|
| POST | `/v1/cluster/add-node` | `crates/valori-node/src/cluster_api.rs:68` |
| POST | `/v1/cluster/remove-node` | `crates/valori-node/src/cluster_api.rs:69` |
| POST | `/v1/cluster/snapshot` | `crates/valori-node/src/cluster_api.rs:70` |
| DELETE | `/v1/crypto/shred/:key_id` | `crates/valori-node/src/server.rs:432` |
| GET | `/v1/keys` | `crates/valori-node/src/server.rs:341` |
| POST | `/v1/keys` | `crates/valori-node/src/server.rs:341` |
| DELETE | `/v1/keys/:id` | `crates/valori-node/src/server.rs:342` |

## OPERATOR_INTERNAL (5)

| Method | Path | Source |
|---|---|---|
| GET | `/metrics` | `crates/valori-node/src/server.rs:336` |
| GET | `/v1/cluster/read-index` | `crates/valori-node/src/cluster_api.rs:66` |
| GET | `/v1/replication/events` | `crates/valori-node/src/server.rs:392` |
| GET | `/v1/replication/state` | `crates/valori-node/src/server.rs:396` |
| GET | `/v1/replication/wal` | `crates/valori-node/src/server.rs:391` |

## DEPRECATED (14)

| Method | Path | Superseded by |
|---|---|---|
| POST | `/graph/edge` | `POST /v1/graph/edge` |
| GET | `/graph/edges/:id` | `GET /v1/graph/edges/{id}` |
| POST | `/graph/node` | `POST /v1/graph/node` |
| DELETE | `/graph/node/:id` | `/v1/graph/node/{id}` |
| GET | `/graph/node/:id` | `/v1/graph/node/{id}` |
| GET | `/graph/nodes` | `GET /v1/graph/nodes` |
| GET | `/graph/subgraph` | `GET /v1/graph/subgraph` |
| GET | `/operations` | `GET /v1/operations` |
| GET | `/operations/:id` | `GET /v1/operations/{id}` |
| POST | `/records` | `POST /v1/records` |
| POST | `/search` | `POST /v1/search` |
| GET | `/timeline` | `GET /v1/timeline` |
| POST | `/v1/vectors/batch_insert` | `POST /v1/vectors/batch-insert` |
| GET | `/version` | `GET /v1/version` |

---

## Regenerating

```bash
python3 scripts/generate-route-manifest.py    # rewrites the manifest
python3 scripts/verify-api-route-contract.py  # proves the boundary holds
```
