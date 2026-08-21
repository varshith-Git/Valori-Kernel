# Security & Authorization Contract — Valori API v1

## Single source of truth

`x-required-scope` in `api/openapi/valori-v1.yaml` is not written by hand and
not transcribed from documentation. `VendorExtensionAddon` in
`crates/valori-node/src/openapi.rs` calls
`crate::api_keys::required_scope(&method, &path)` — **the same function the
auth middleware calls on every request** — and writes its result into the
document during generation.

The contract therefore cannot claim a scope the server does not enforce: both
values come from one function, and drift between them is not expressible.

## Authentication

One scheme, `BearerAuth` (HTTP bearer), declared once by `SecurityAddon`:

    Authorization: Bearer <project API key | VALORI_AUTH_TOKEN>

A project API key is looked up in the `KeyStore` and carries a scope. The
legacy `VALORI_AUTH_TOKEN` is compared in constant time and, when it matches,
satisfies any scope.

If the node has no auth configured at all (`has_any_auth() == false`), the
middleware passes every request through — a deployment choice, not a contract
tier.

## Per-operation security

| Operations | `security` | Meaning |
|---|---|---|
| 73 | `[{"BearerAuth": []}]` | Credentials required |
| 1 (`GET /health`) | `[]` | No credentials consulted |

`GET /health` is deliberately unauthenticated so load-balancer and container
probes work without a token. It is also why it is the one operation in the
contract with no 4xx — see `redocly.yaml`.

## Scopes

`ApiScope::satisfies` is a hierarchy: `admin` satisfies `read_write`, which
satisfies `read_only`.

| Scope | Public operations |
|---|---|
| `read_only` | 36 |
| `read_write` | 28 |
| `admin` | 10 |

Note that `admin` is **not** confined to the non-public `ADMIN` routes. Ten
operations that are in the public SDK contract require an admin-scoped key,
because `required_scope` maps the whole `/v1/snapshot/*` and `/v1/storage/*`
prefixes to `admin`: they read or replace entire-node state and are not
namespace-scoped, so a tenant-level read/write key must not reach them.

### Public operations requiring `admin`

| Method | Path | operationId |
|---|---|---|
| GET | `/v1/snapshot/download` | `download_snapshot` |
| POST | `/v1/snapshot/restore` | `restore_snapshot` |
| POST | `/v1/snapshot/save` | `save_snapshot` |
| POST | `/v1/snapshot/upload` | `upload_snapshot` |
| GET | `/v1/storage/manifest` | `get_storage_manifest` |
| GET | `/v1/storage/snapshots` | `list_object_store_snapshots` |
| POST | `/v1/storage/snapshots/restore` | `restore_snapshot_from_object_store` |
| POST | `/v1/storage/snapshots/upload` | `upload_snapshot_to_object_store` |
| GET | `/v1/storage/wal` | `list_archived_wal_segments` |
| POST | `/v1/storage/wal/archive` | `archive_wal_segment` |

An SDK generated from this contract can present these as admin-only without
consulting any prose: the scope is on the operation.

## Failure responses

Both come from the middleware, before any handler runs, and both are returned
by axum as a bare status with **an empty body** — not `ApiError`:

| Status | Condition |
|---|---|
| 401 | No `Authorization: Bearer` header, or an unrecognised token |
| 403 | Token authenticated, but its scope does not satisfy `x-required-scope` |

`AuthResponsesAddon` in `crates/valori-node/src/openapi.rs` attaches both to
every operation carrying a non-empty `security` requirement, from that single
place. Before Phase API-3.2 the contract declared `401` with `body = ApiError`
on 70 hand-written annotations — a body that never existed — and documented
`403` on none of them.
