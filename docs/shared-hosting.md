# Shared free-tier hosting

One `valori-node` process can host multiple isolated projects. Each project owns
an independent `Engine`, collection registry, graph, metadata, execution/receipt
registries, and event log. CPU, memory, the HTTP listener, and the process are
shared; this is not a container, process, or cgroup per free project.

The Cloud control plane chooses shared hosting for **new free projects**
(SH2, `backend/apps/api/src/main.rs`'s `provision_shared_project`, branching
on `runtime_profiles.hosting_mode` — see `valori-ui`'s
`docs/phases/phase-SH2-cloud-shared-free-provisioning.md`) and keeps the
existing dedicated-container provisioner for paid plans. This is
implemented and locally verified as of SH2; it is **not yet deployed to
the production Azure environment** — see that phase doc's Azure section
for the exact deployment steps and required approval gate. Before SH2,
this line was aspirational documentation with no corresponding
control-plane code (see `docs/reviews/shared-free-hosting-live-verification.md`,
the audit that found the gap). Existing dedicated projects, including
legacy free ones, are not moved automatically.

## Worker configuration

Run the existing `valori-node` binary with:

- `VALORI_SHARED_ROOT`: a dedicated, persistent directory, e.g. `/data/shared`.
- `VALORI_SHARED_ADMIN_TOKEN`: an operator secret of at least 32 characters.
- `VALORI_SHARED_MAX_PROJECTS`: maximum registered projects; default `100`.
- `VALORI_BIND`: the worker listener, e.g. `0.0.0.0:3000`.

**Persistent volume mount point (Docker):** the production image runs as a
non-root user (UID/GID `65532`, distroless `nonroot`) and pre-owns exactly
one directory in the image, `/data`, so Docker's own "seed a fresh named
volume from the image path it's mounted over" behavior gives that volume
correct ownership automatically. **Mount your persistent volume at `/data`,
not at `/data/shared`** — a volume mounted directly at `/data/shared` has no
matching pre-owned image path to seed from, so Docker creates it root-owned
and the non-root process cannot write to it (`PUT /shared/projects/{id}`
fails with a 500 wrapping "Permission denied", confirmed live in
`docs/reviews/shared-free-hosting-live-verification.md`). Set
`VALORI_SHARED_ROOT=/data/shared`; the worker process itself creates that
subdirectory inside the (correctly-owned) `/data` volume on first use via
`SharedHost::open()`'s own `create_dir_all`. Do not `chmod 777` the volume
and do not run the container as root — neither is necessary once the mount
point is correct, and both were explicitly rejected as fixes for this
issue.

The shared root contains `<project-uuid>/project.json`, `events.log`, and
project-specific sidecars. Only the hash of each project's worker token is
stored in its manifest. The root must be owned by one shared worker process.
Use a persistent disk and back it up at the operator level. The shared worker
does not inherit another project's event log, object-store prefix, or embedding
credentials. It rejects a simultaneous `VALORI_CLUSTER_MEMBERS` configuration.

The worker can be a single long-running container or a service on the existing
worker VM. Creating a free project never creates another container.

## Private control-plane API

All management requests require `Authorization: Bearer <shared-admin-token>`.
Do not expose this credential to the browser or customers.

- `PUT /shared/projects/{uuid}` with `worker_auth_token` and `max_records`:
  idempotently register a project. Conflicting retries and capacity overflow
  return `409`. Missing management credentials return `401`.
- `PUT /shared/projects/{uuid}/active` with `{"active":false|true}`:
  stop/resume that project without touching other projects or the process.
- `DELETE /shared/projects/{uuid}`: remove only that project's state and files.
- `GET /shared/projects/{uuid}/export`: export a stopped project's durable
  event log, sidecars, and expected state hash for a paid upgrade.

Cloud stores the project's node URL as
`http://worker:3000/shared/projects/{uuid}/node`. Its existing proxy appends
normal paths such as `/v1/namespaces`, `/v1/records`, and `/v1/search`. Every
request, including project health, must carry **that project's** worker token.
An A token cannot address B, even if both have a collection named `documents`
and both contain record ID 0. Collection names need no hidden prefixes.

## Persistence and limits

Shared mode flushes events before acknowledging requests and validates logs
before accepting recovered projects. A corrupt project log stops shared-worker
startup rather than quietly replacing the project with empty state. There is
no shared-mode log rotation yet: keep the persistent disk monitored.

Normal synchronous record, search, graph, collection, metadata, and proof APIs
reuse the standalone router. The following operations are unavailable in shared
mode: global metrics, node API-key management, local model inventory, object
storage/replication administration, filesystem snapshot save/restore, snapshot
upload, encryption/key shredding, background index management, and full on-node
ingestion. Client-side embedding plus vector insert remains available, as does
stateless document chunking. These restrictions do not change dedicated nodes.

Free projects share a failure domain and CPU/memory budget; this is logical data
isolation, not per-project OS resource isolation. The configured project-count
bound is an admission limit, not an Azure capacity benchmark.

## Paid transition

The Cloud billing callback invokes the admin-only
`POST /v1/admin/orgs/{org-id}/dedicate` control-plane endpoint. The backend reads
the real plan from PostgreSQL, serializes provisioning per project, pauses the
shared project, exports its log, provisions through the existing dedicated
path, and imports through `POST /internal/shared-import` on the destination.
The import is admin-scoped and standalone-only: it cannot bypass Raft on a
cluster. The normal public SDK/API surface is unchanged.

The destination replays and verifies the log and writes the transferred data
durably. Cloud switches `projects.node_url` only after the destination reports
the expected state hash. Source cleanup follows cutover; a cleanup retry never
reimports over new paid-project writes. Failed transfers retain the source.
Stopped/suspended paid projects transition when started. Downgrades do not
automatically move existing dedicated data back to shared storage.

The transfer API has a 256 MiB encoded request limit. Oversized transfers fail
without switching the project's URL; streaming large transfers is future work.
Transient caches and historical in-memory execution receipts are not copied.
The event log and its cryptographic audit history are copied.

## Rollout

Deploy the updated node binary to the shared worker and use the same release for
new dedicated containers. Apply Cloud migration `0021_shared_projects.sql` and
configure `SHARED_WORKER_URL`, `SHARED_WORKER_ADMIN_TOKEN`, and
`SHARED_WORKER_REGION` on the control plane. The URL must be an HTTP(S) origin,
without credentials, path, query, or fragment. All three settings are required
together. With no shared worker configured, new free provisioning fails closed;
it does not silently fall back to a paid-style container.

Before enabling signup, exercise two free accounts with the same collection
name, verify cross-project access fails, restart the shared worker, stop/delete
one project, and upgrade the other using a test subscription. Confirm the audit
proof and data survive a restart of its new dedicated container. No deployment
or live account migration is performed by the code change itself.
