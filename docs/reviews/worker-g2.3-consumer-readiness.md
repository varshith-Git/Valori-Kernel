# Consumer Worker Readiness (G2.3)

Source-code audit only. No code, config, or infrastructure was changed to
produce this document. All claims are traced to specific files/lines in
`valori-ui` (private control plane) and `Valori-Kernel` (public node
runtime) as they exist on disk today. Labels used throughout:

- **FACT** — directly observed in source, with citation.
- **OBSERVATION** — a pattern/absence noticed across multiple files.
- **HYPOTHESIS** — a plausible inference not directly provable from code.
- **DECISION** — this audit's judgment call given the above.

---

## Environment

FACT — Given by the task, not verified by this audit (no Azure access was
used): control plane `valori-control-plane-01` (172.16.0.4, public
`https://api.valori.systems`), worker `valori-worker-01` (172.16.0.5), both
on `vnet-southindia-1` / `snet-southindia-1`, private ICMP connectivity
confirmed 3/3.

---

## Control Plane Architecture

FACT — `valori-ui/backend/apps/api` is an axum HTTP service backed by
Postgres (`sqlx::PgPool`). It owns two schemas on one Postgres instance:
`infra` (hosts/instances/worker_tokens — this service's own data) and
reads/writes `public.projects` in Supabase's schema for user-facing project
state ([`models.rs:1-7`](../../../../valori-ui/backend/apps/api/src/models.rs)).

FACT — Layering, per the module doc comment: `Provisioner`/HTTP handlers →
`WorkerService` → `placement::place()` (pure) → `db::host` (repository)
([`worker_service.rs:1-14`](../../../../valori-ui/backend/apps/api/src/worker_service.rs)).

---

## Worker Registry

| Component | File:Line | Purpose | Actual behavior |
|---|---|---|---|
| `infra.hosts` table | `backend/migrations/0001_hosts_and_instances.sql`, extended by `0004_worker_fields.sql`, `0007_worker_heartbeat_richer.sql`, `0015_host_docker_endpoint.sql`, `0017_host_worker_class.sql` | Worker registry schema | Columns: `id, region, worker_class, provider, ip, dokploy_url, docker_host, capacity_slots, used_slots, status, hostname, agent_version, last_seen, cpu_pct, memory_pct, disk_pct, container_count, uptime_seconds, reported_project_count, reported_available_slots` ([`db/host.rs:17-19`](../../../../valori-ui/backend/apps/api/src/db/host.rs)) |
| `Host` struct | [`models.rs:73-106`](../../../../valori-ui/backend/apps/api/src/models.rs) | Rust mapping of the row | 1:1 with the columns above |
| `HostStatus` | [`models.rs:12-23`](../../../../valori-ui/backend/apps/api/src/models.rs) | Administrative intent | `Active \| Draining \| Offline \| Maintenance` — set only by an operator/admin route, never by the worker itself |
| `WorkerHealth` | [`models.rs:33-47`](../../../../valori-ui/backend/apps/api/src/models.rs) | Observed liveness | Computed fresh every read from `status` + `last_seen` age (`Host::health()`, [`models.rs:113-131`](../../../../valori-ui/backend/apps/api/src/models.rs)); never persisted, never trusted from the worker's own claim |
| `WorkerService` | [`worker_service.rs:53-176`](../../../../valori-ui/backend/apps/api/src/worker_service.rs) | The one place worker/placement decisions are made | `register`, `heartbeat`, `find_available`, `reserve_slot`, `mark_draining`/`set_maintenance`/`disable`/`enable`, `mark_dead`, `set_status` |
| `infra.worker_tokens` | `backend/migrations/0006_worker_tokens.sql`; queries in [`db/worker_token.rs`](../../../../valori-ui/backend/apps/api/src/db/worker_token.rs) | Per-worker auth credential | `wtk_`-prefixed token, SHA-256-hashed at rest, one row per token, revocable individually ([`db/worker_token.rs:14-93`](../../../../valori-ui/backend/apps/api/src/db/worker_token.rs)) |
| `WorkerAuth` extractor | [`auth/worker.rs:44-70`](../../../../valori-ui/backend/apps/api/src/auth/worker.rs) | Authenticates `POST /v1/internal/heartbeat` | Bearer token → `verify_and_touch` → `host_id`; **the file's own header comment states no real worker-heartbeat agent is deployed anywhere yet** ([`auth/worker.rs:8-10`](../../../../valori-ui/backend/apps/api/src/auth/worker.rs)) |
| Admin host CRUD | [`main.rs:400-404`](../../../../valori-ui/backend/apps/api/src/main.rs) | Worker registration/decommission | `GET/POST /v1/admin/hosts`, `PATCH/DELETE /v1/admin/hosts/:id`, `POST /v1/admin/hosts/:id/mark-dead`, token issue/list/revoke under `/v1/admin/hosts/:id/tokens[/:token_id]` |
| `db::host::delete` | [`db/host.rs:171-183`](../../../../valori-ui/backend/apps/api/src/db/host.rs) | Decommissioning | Refuses to delete a host with `used_slots > 0` (`DeleteHostError::InUse`) |
| `WorkerService::mark_dead` | [`worker_service.rs:120-165`](../../../../valori-ui/backend/apps/api/src/worker_service.rs) | Disaster path for a host that died with running instances | Deletes its `infra.instances` rows, zeroes `used_slots`, sets status Offline, flags any now-fully-down project `error` in Supabase, audit-logs the event |
| Region/provider/class fields | `region`, `provider`, `worker_class` columns ([`db/host.rs:80-90`](../../../../valori-ui/backend/apps/api/src/db/host.rs)) | Placement inputs | Free-text strings, set at host creation; `worker_class` defaults `'standard'` (`backend/migrations/0017_host_worker_class.sql:15`) |

---

## Registration

**FACT** — Registration is **push from an operator into the control
plane's own database**, not a call initiated by the worker, and not
automatic. `WorkerService::register` ([`worker_service.rs:62-64`](../../../../valori-ui/backend/apps/api/src/worker_service.rs))
is a thin wrapper over `db::host::create`, invoked by `POST
/v1/admin/hosts` ([`main.rs:400`](../../../../valori-ui/backend/apps/api/src/main.rs)), gated by `AdminAuth` (`x-admin-key`).

**Exact endpoint (real, not invented):**
```
POST /v1/admin/hosts
```
Body maps to `NewHost { region, worker_class, provider, ip, dokploy_url,
docker_host, capacity_slots, hostname }` ([`db/host.rs:80-90`](../../../../valori-ui/backend/apps/api/src/db/host.rs)).

**Answers:**
- Does a worker call the control plane to register? **No.** No such client
  exists (see Worker Agent below).
- Does the control plane call the worker to register it? **No** — it only
  writes a database row; it never contacts the IP at registration time.
- Registration mode: **manual, API-driven, admin-key-authenticated.** Not
  push-from-worker, not token-driven (the worker token is issued
  *separately*, after the host row already exists — `POST
  /v1/admin/hosts/:id/tokens`), not automatic.
- There is no `POST /v1/workers/register` or any self-registration
  endpoint anywhere in the codebase. Only `/v1/admin/hosts` (operator →
  registry) and `/v1/internal/heartbeat` (worker → registry, post-hoc)
  exist.

---

## Heartbeat

**Endpoint:** `POST /v1/internal/heartbeat` ([`main.rs:399`](../../../../valori-ui/backend/apps/api/src/main.rs), handler `worker_heartbeat` at [`main.rs:1468-1495`](../../../../valori-ui/backend/apps/api/src/main.rs)).

| Aspect | Detail |
|---|---|
| Request body | `HeartbeatBody { cpu_pct, memory_pct, disk_pct, container_count, uptime_seconds, agent_version: Option<String>, reported_project_count, reported_available_slots }` ([`main.rs:1448-1466`](../../../../valori-ui/backend/apps/api/src/main.rs)). Deliberately **no** `status`/`healthy` field. |
| Authentication | `Authorization: Bearer <per-worker wtk_ token>`, via `WorkerAuth` extractor ([`auth/worker.rs:44-70`](../../../../valori-ui/backend/apps/api/src/auth/worker.rs)) |
| Worker identity | Derived server-side from the token lookup (`host_id`), **never** taken from the request body — comment explicitly calls this out as the anti-forgery property ([`auth/worker.rs:41-46`](../../../../valori-ui/backend/apps/api/src/auth/worker.rs)) |
| Status | Not accepted from the worker; `WorkerHealth` is always derived server-side from `status` + `last_seen` age ([`models.rs:113-131`](../../../../valori-ui/backend/apps/api/src/models.rs)) |
| Capacity | `reported_project_count`/`reported_available_slots` stored but explicitly **not** used to overwrite `capacity_slots`/`used_slots` — comment: "a persistent mismatch is a useful signal on its own... not something to silently overwrite" ([`db/host.rs:99-105`](../../../../valori-ui/backend/apps/api/src/db/host.rs)) |
| Timestamp | `last_seen = now()` set server-side on write ([`db/host.rs:141`](../../../../valori-ui/backend/apps/api/src/db/host.rs)) |
| Response | `204 No Content` on success |
| Failure behavior | Bad/missing/revoked token → `401`; DB error → `500` ([`auth/worker.rs:31-39`](../../../../valori-ui/backend/apps/api/src/auth/worker.rs)); unknown `host_id` → `WorkerServiceError::NotFound` |
| Not audited | Deliberately excluded from `audit_logs` — heartbeats fire ~30s and would drown discrete events ([`main.rs:1473-1476`](../../../../valori-ui/backend/apps/api/src/main.rs)) |

**Does a worker-side process already exist that calls it?** **No.** Both
`auth/worker.rs:8-10` and `main.rs:1443-1445` carry the identical explicit
comment: *"no real worker-heartbeat agent is deployed anywhere yet... this
route is genuine, tested groundwork, not evidence anything is heartbeating
in production today."*

---

## Worker Agent

**FACT** — Grepping both repos for worker agent / daemon / heartbeat
client / registration client / bootstrap / host agent / capacity reporter
turns up:
- `valori-ui/backend/apps/api/src/deployment/agent.rs` — this is a
  **`DeploymentAgent` trait for rolling out node *version upgrades***, not
  a registration/heartbeat agent. Its own doc comment: *"There is no real
  Worker Agent to call yet — no daemon on any real machine listening for a
  deploy command... `MockDeploymentAgent` is what's wired in today"*
  ([`deployment/agent.rs:5-14, 38-48`](../../../../valori-ui/backend/apps/api/src/deployment/agent.rs)).
- `crates/valori-models/src/health.rs` — one unrelated string match
  (`Vec::with_capacity`), not a capacity reporter.
- No file in either repo implements an executable/daemon that: authenticates
  as a worker, calls `POST /v1/internal/heartbeat`, or calls any
  self-registration endpoint.

**Classification: MISSING.** The control-plane side (`WorkerService`,
`WorkerAuth`, the heartbeat route, the worker-token store) is real,
tested, wired-up code — but it is a server waiting for a client that has
never been written. Nothing on a fresh VM — Azure or otherwise — currently
talks to it. This directly answers the task's framing: **"we already have
the Cloud-side WorkerService... nothing on a fresh VM actually talks to
it"** — confirmed true by source.

---

## Provisioning

Traced `provision_project` end-to-end, all in `valori-ui/backend/apps/api/src`:

1. **API endpoint** — handler `provision_project`, route wired in
   `main.rs` (search hit at [`main.rs:705`](../../../../valori-ui/backend/apps/api/src/main.rs)), gated by `AuthUser` (customer JWT), scoped to a project the caller owns via Supabase RLS.
2. **Control-plane handler** — `provision_project` ([`main.rs:705-768`](../../../../valori-ui/backend/apps/api/src/main.rs)) looks up the project via Supabase, calls `provision_project_inner`, then audit-logs + emits a notification + sends a success/failure email — none of which block the actual provisioning result.
3. **`WorkerService` call** — `provision_project_inner` ([`main.rs:860-`](../../../../valori-ui/backend/apps/api/src/main.rs)) calls `check_quota_and_entitlements` (plan limits, `RuntimeLimits`), resolves a per-project `worker_auth_token`, then `state.worker_service.find_available(region, plan.worker_class(), replication)` ([`main.rs:877-880`](../../../../valori-ui/backend/apps/api/src/main.rs)).
4. **Worker selection** — `WorkerService::find_available` ([`worker_service.rs:82-90`](../../../../valori-ui/backend/apps/api/src/worker_service.rs)) reads `list_active_by_region` (status = `'active'` only — **health/heartbeat freshness is not a placement filter**), then calls the pure `placement::place()` ([`placement.rs:35-64`](../../../../valori-ui/backend/apps/api/src/provision/placement.rs)): exact region + exact `worker_class` match, `status = Active`, `free_slots() > 0`, sorted by most-free-first.
5. **Deployment instruction** — for each chosen host, `provision_project_inner` optionally provisions a managed volume (`VolumeService`, only if `Provisioner::managed_volume_kind()` returns `Some`), then calls `state.provisioner.deploy(host, &DeployRequest{...})` ([`main.rs:900-919`](../../../../valori-ui/backend/apps/api/src/main.rs)) — retried up to `MAX_PORT_COLLISION_RETRIES = 5` on a unique-constraint collision.
6. **Node creation** — which `Provisioner` impl runs depends on `PROVISIONER` env var, default **`Mock`** ([`config.rs:229`](../../../../valori-ui/backend/apps/api/src/config.rs): `_ => Some(ProvisionerKind::Mock), // default until a real host exists`), with `docker` and `dokploy` as the two real options ([`main.rs:206-239`](../../../../valori-ui/backend/apps/api/src/main.rs)). `PROVISIONER=mock` logs a startup warning: *"no real infrastructure will be touched"* ([`main.rs:220`](../../../../valori-ui/backend/apps/api/src/main.rs)).
7. **Node startup** — under `PROVISIONER=docker`, `DockerProvisioner::deploy` ([`provision/docker.rs:346-455`](../../../../valori-ui/backend/apps/api/src/provision/docker.rs)) talks **directly to the target host's Docker Engine API** (default `http://{ip}:2375`, or `host.docker_host` if set) — pulls the image, creates a named volume or mounts the orchestrator-provided one, creates a container named `valori-{project_id}-{node_index}` with env `VALORI_BIND, VALORI_DIM, VALORI_INDEX, VALORI_MAX_RECORDS, VALORI_SNAPSHOT_PATH=/data/snapshot.bin, VALORI_EVENT_LOG_PATH=/data/events.log, VALORI_AUTH_TOKEN`, starts it, reads back the Docker-assigned host port. Under `PROVISIONER=dokploy`, the analogous path goes through the Dokploy API instead (`dokploy.rs`, not detailed further here — its own doc comment flags the exposed-port behavior as unverified).
8. **Health check** — `DockerProvisioner::status`/`inspect` map container state (`running`/`created`/`restarting`/`exited`/etc.) to `InstanceStatus`; the node itself exposes `GET /health` ([`crates/valori-node/src/server.rs:534-566`](../../../../crates/valori-node/src/server.rs), unauthenticated by design so a load balancer can probe it) — but nothing in `provision_project_inner` currently polls `/health` before returning; readiness is inferred from the container-create/start HTTP calls succeeding, not from the node's own liveness endpoint. *(OBSERVATION, not exhaustively re-verified beyond the code read above — instance-lifecycle blue/green cutover code may poll it elsewhere, out of scope for this pass.)*
9. **Project readiness** — on success, `db::instance::insert` records the `DeployedNode` (host_id, container_id, http_port), and for `docker`/`dokploy` provisioners a Caddy route is registered ([`caddy_router.rs`](../../../../valori-ui/backend/apps/api/src/provision/caddy_router.rs)) so `https://<id>.nodes.valori.systems` resolves; `MockProvisioner` gets no such route (`uses_dns_routing` gate, [`main.rs:246`](../../../../valori-ui/backend/apps/api/src/main.rs)).

**Sequence (docker path):**
```
Customer -> POST /v1/project/:id/provision (AuthUser)
  -> provision_project_inner
    -> check_quota_and_entitlements (billing)
    -> resolve_worker_auth_token
    -> WorkerService::find_available -> placement::place (pure, DB-free)
    -> [optional VolumeService.create]
    -> Provisioner::deploy
         (docker)   -> Docker Engine API on worker: pull, create volume,
                        create container, start, inspect for host port
         (dokploy)  -> Dokploy API
         (mock)     -> in-memory fake, touches nothing real
    -> db::instance::insert (persists DeployedNode)
    -> [docker/dokploy only] CaddyRouter.add_route
  -> audit log + notification + email (best-effort)
```

**FACT — CRITICAL: no step in this sequence calls a worker agent, calls
the heartbeat endpoint, or requires the worker to have registered/
heartbeated first.** Placement only checks `HostStatus::Active` +
`free_slots() > 0`. A host that has never heartbeated (`WorkerHealth::
Unknown`) is still eligible for placement — health is a display concern,
not a scheduling gate.

---

## Node Runtime

Inspected `Valori-Kernel` (public repo).

**FACT** — Distribution is a **statically-linked Rust binary in a
multi-stage distroless Docker image** ([`Dockerfile:1-9`](../../../Dockerfile)):
stage 1 builds with `rust:slim-bookworm`, stage 2 runs on Google distroless
(no shell, minimal CVE surface). No systemd unit, no embedded-process mode,
no non-Docker packaging is defined in this repo for the node binary.

| Item | Value | Source |
|---|---|---|
| Startup command | `docker run -p 3000:3000 -v $(pwd)/data:/data valori-node:latest` (documented pattern; actual command from Cloud is via Docker Engine API `POST /containers/create` + `/start`, not a CLI) | [`Dockerfile:8`](../../../Dockerfile) |
| Bind address | `VALORI_BIND` (default `0.0.0.0:3000`) | [`crates/valori-node/src/config.rs:266`](../../../crates/valori-node/src/config.rs) |
| Data directory | `/data` (Cloud's convention — see `DATA_MOUNT` in `docker.rs`); node itself has no single "data dir" concept, only independent path env vars | [`provision/docker.rs:61`](../../../../valori-ui/backend/apps/api/src/provision/docker.rs) |
| WAL / event log path | `VALORI_EVENT_LOG_PATH` — Cloud sets `/data/events.log` | [`config.rs:378`](../../../crates/valori-node/src/config.rs), [`docker.rs:127`](../../../../valori-ui/backend/apps/api/src/provision/docker.rs) |
| Snapshot path | `VALORI_SNAPSHOT_PATH` — Cloud sets `/data/snapshot.bin` | [`config.rs:294`](../../../crates/valori-node/src/config.rs), [`docker.rs:126`](../../../../valori-ui/backend/apps/api/src/provision/docker.rs) |
| Port | Container always listens `3000/tcp` internally; host-side port is Docker-assigned dynamically (empty `HostPort` in the bind spec, read back from `containers/{id}/json`) | [`docker.rs:57, 237-240, 436-446`](../../../../valori-ui/backend/apps/api/src/provision/docker.rs) |
| Health endpoint | `GET /health` — always unauthenticated by design | [`crates/valori-node/src/server.rs:534-566`](../../../crates/valori-node/src/server.rs) |
| Index configuration | `VALORI_INDEX` (`brute`/`hnsw`/`ivf`/`bq`/`auto`) — Cloud passes `req.index` through | [`docker.rs:124`](../../../../valori-ui/backend/apps/api/src/provision/docker.rs); documented env table in [`CLAUDE.md`](../../CLAUDE.md) |
| Required env vars set by Cloud | `VALORI_BIND, VALORI_DIM, VALORI_INDEX, VALORI_MAX_RECORDS, VALORI_SNAPSHOT_PATH, VALORI_EVENT_LOG_PATH, VALORI_AUTH_TOKEN`, plus `VALORI_OBJECT_STORE_*`/`AWS_*` when object storage is configured | [`docker.rs:120-153`](../../../../valori-ui/backend/apps/api/src/provision/docker.rs) |
| Logging | Not specifically configured by the provisioner (no `RUST_LOG`/log-driver env set in `build_env`) | [`docker.rs:120-154`](../../../../valori-ui/backend/apps/api/src/provision/docker.rs) — OBSERVATION: absence, not a positive claim |

---

## Storage

**FACT** — Every deployed node writes to its own **local container
filesystem path** (`/data`, backed by a Docker named volume when
`PROVISIONER=docker`): `VALORI_SNAPSHOT_PATH`, `VALORI_EVENT_LOG_PATH`.
Records, graph, and index all live inside the single process's in-memory
`KernelState`, persisted only via snapshot + WAL at those two paths — this
matches the architecture described in `CLAUDE.md`'s snapshot/WAL model, no
kernel-repo code inspected in this pass contradicts it.

**FACT** — Object-store offload exists and is optional, gated by
`VALORI_OBJECT_STORE_URL` (`s3://`, `b2://`, or `file://`) per the env
table in `CLAUDE.md` and confirmed wired from the Cloud side: `docker.rs`
only emits `VALORI_OBJECT_STORE_*`/`AWS_*` env vars **if** `self.object_store`
is `Some` ([`docker.rs:131-151`](../../../../valori-ui/backend/apps/api/src/provision/docker.rs)). If `NODE_OBJECT_STORE_BUCKET` isn't set on
the control plane, it logs a startup warning and every deployed node runs
**local-disk-only, no S3/R2/MinIO durability** ([`main.rs:211-217`](../../../../valori-ui/backend/apps/api/src/main.rs)).

**Do not infer beyond this**: whether `valori-worker-01` specifically has
any object store reachable, or whether Cloud's `NODE_OBJECT_STORE_BUCKET`
is currently set, was not observable from source and is not claimed here.

---

## Multi-Project Isolation

**FACT** — Two separate isolation mechanisms exist at two separate layers,
and they are not interchangeable:

1. **Collection/namespace isolation** — *within one node process*.
   Per `CLAUDE.md`'s architecture section: namespaces are 16-bit
   `NamespaceId`s (`MAX_NAMESPACES = 1024`), each with intrusive linked
   record lists inside one `KernelState`. A Collection now owns
   `dimension`/`metric`/`index` (per the task's stated recent change).
   This is isolation **within a single project's node(s)** — Project A's
   Collection A and Collection B share the same process, same port, same
   data directory, same WAL, same snapshot file.

2. **Project isolation** — *at the process/container level*, enforced
   entirely by the Cloud provisioning layer, not by the kernel. Each
   `DeployRequest` produces one container named `valori-{project_id}-
   {node_index}` ([`docker.rs:386`](../../../../valori-ui/backend/apps/api/src/provision/docker.rs)), with:
   - its own Docker-assigned host port ([`docker.rs:394-395, 436-446`](../../../../valori-ui/backend/apps/api/src/provision/docker.rs))
   - its own named volume, `valori-data-{project_id}-{node_index}`
     ([`docker.rs:156-158`](../../../../valori-ui/backend/apps/api/src/provision/docker.rs)) or a `VolumeService`-managed one
   - its own `/data/snapshot.bin` and `/data/events.log` inside that volume
   - its own `VALORI_DIM` (permanent per-project, per `traits.rs:17-21`)
   - its own `VALORI_AUTH_TOKEN` (per-project `worker_auth_token`, see
     `resolve_worker_auth_token`, [`main.rs:823-858`](../../../../valori-ui/backend/apps/api/src/main.rs))

**Answer to the stated topology (Worker-01 running Projects A/B/C, each
with their own Collections):** the CURRENT implementation supports this
**only through container-per-project isolation**, which is real, tested
code in `DockerProvisioner`/`DokployProvisioner` — nothing in the kernel
process itself provides cross-project isolation (there is exactly one
`KernelState`, one namespace registry, one `VALORI_DIM` per process). So:

- **namespace**: isolates Collections within one Project — not a
  cross-project boundary.
- **project**: isolates via a dedicated container — real, code-complete.
- **process**: 1 process per Project (per node/replica) — real.
- **port**: Docker-assigned, distinct per container — real.
- **data directory**: distinct named volume per `(project_id, node_index)`
  — real.
- **index / snapshot / WAL**: all scoped inside that per-project volume —
  real, by construction of the isolation above (one process, one data dir).

**DECISION** — this boundary is architecturally sound *on paper* but its
`docker.rs` path carries an explicit, un-removed caveat: *"Unverified
against a live Docker host... structurally correct against Docker's own
published API docs, not yet run for real"* ([`docker.rs:15-17`](../../../../valori-ui/backend/apps/api/src/provision/docker.rs)). Treat
container-per-project isolation as **designed, not demonstrated**.

---

## Networking

FACT/OBSERVATION (topology requirements derived from the code paths
above, not from any Azure NSG inspected):

| Direction | Port(s) | Why |
|---|---|---|
| Control plane → Worker | `2375` (plain HTTP, default) or `2376`/custom (`docker_host`, TLS) | `DockerProvisioner` talks to the Docker Engine API directly ([`docker.rs:83-91`](../../../../valori-ui/backend/apps/api/src/provision/docker.rs)) |
| Control plane → Worker | `2019` (Caddy admin API default) | Route registration, if `docker`/`dokploy` provisioning with DNS routing is used ([`caddy_router.rs:56-60`](../../../../valori-ui/backend/apps/api/src/provision/caddy_router.rs)) |
| Control plane → Worker | Whatever host port Docker assigns per container (health checks/`status()`/instance ops go through the Docker API, not directly to the node) | `docker.rs` `inspect`/`stop`/`start` |
| Worker → Control plane | `443` on `api.valori.systems`, `POST /v1/internal/heartbeat` | **Only if/when a worker agent is ever built** — nothing today initiates this |
| Customer → Worker | The node's public route (`https://<id>.nodes.valori.systems`, proxied via Caddy on the worker) OR direct `host:port` for a Mock/undocumented path | `caddy_router.rs` route registration |

**Must remain private:** the Docker Engine API port (2375/2376) and the
Caddy admin API port (2019) — both modules' own doc comments say plain
HTTP has **no authentication at all** and must never be internet-facing
([`docker.rs:32-42`](../../../../valori-ui/backend/apps/api/src/provision/docker.rs), [`caddy_router.rs:35-43`](../../../../valori-ui/backend/apps/api/src/provision/caddy_router.rs)).

**Should customer traffic reach the worker directly?** By design, no — it
should terminate at Caddy's public route on the worker host and be proxied
to the container's Docker-assigned port; the Docker Engine API and Caddy
admin API must stay private-VNet-only. The customer-facing node port itself
is expected to be reachable (that's the whole point of the route), just
not the two management APIs.

The 172.16.0.4 ⇄ 172.16.0.5 private connectivity already proven (ICMP)
does not by itself prove any of the above TCP ports are open — ping success
says nothing about port reachability.

---

## Security

| Item | Status | Source |
|---|---|---|
| Worker identity | `host_id` (UUID), resolved server-side from a bearer token — never client-supplied | [`auth/worker.rs:41-46`](../../../../valori-ui/backend/apps/api/src/auth/worker.rs) |
| Worker token | `wtk_`-prefixed, 32 random bytes hex-encoded | [`db/worker_token.rs:14-26`](../../../../valori-ui/backend/apps/api/src/db/worker_token.rs) |
| Token storage | SHA-256 hash only, plaintext shown once at creation, same contract as public API keys | [`db/worker_token.rs:1-7, 40-55`](../../../../valori-ui/backend/apps/api/src/db/worker_token.rs) |
| Token rotation | Manual: `POST .../tokens` to issue a new one, `DELETE .../tokens/:token_id` to revoke; no automatic rotation | [`db/worker_token.rs:87-93`](../../../../valori-ui/backend/apps/api/src/db/worker_token.rs), route list at [`main.rs:403-404`](../../../../valori-ui/backend/apps/api/src/main.rs) |
| TLS requirements | Not enforced by code for Docker Engine API or Caddy admin API — both explicitly documented as the operator's infrastructure responsibility, not something the code checks | [`docker.rs:32-42`](../../../../valori-ui/backend/apps/api/src/provision/docker.rs), [`caddy_router.rs:35-43`](../../../../valori-ui/backend/apps/api/src/provision/caddy_router.rs) |
| Internal auth (heartbeat) | Per-worker Bearer token, real and tested, but nothing sends it yet | [`auth/worker.rs`](../../../../valori-ui/backend/apps/api/src/auth/worker.rs) |
| Admin auth | `x-admin-key` header (`AdminAuth`/`AdminKey`), distinct model from customer JWT auth, gates all `/v1/admin/hosts*` routes | [`main.rs:400-404`](../../../../valori-ui/backend/apps/api/src/main.rs) |
| Project/node auth | `VALORI_AUTH_TOKEN` set per-project (`projects.worker_auth_token`), read by the kernel's own legacy `auth_guard_v2` — before this field was threaded through, every Cloud-provisioned node ran **fully unauthenticated at the node level** (explicit comment, [`traits.rs:55-65`](../../../../valori-ui/backend/apps/api/src/provision/traits.rs)) |

**HTTP-private-VNet vs. HTTPS:** the Docker Engine API defaults to **plain
HTTP with zero authentication** unless the operator sets `docker_host` to
an `https://` TLS endpoint themselves ([`docker.rs:36-42`](../../../../valori-ui/backend/apps/api/src/provision/docker.rs)). Nothing in
code enforces HTTPS. Given same-VNet/same-subnet private connectivity
(172.16.0.4 ↔ 172.16.0.5) with no public exposure, plain HTTP over the
private link is what the code as-shipped assumes/tolerates — but this is
an infrastructure choice the operator must make and secure, not something
the control plane verifies or requires.

---

## Capacity

**FACT** — Two entirely separate capacity concepts exist, and they are
**not reconciled automatically**:

1. **Placement capacity** (`capacity_slots`, `used_slots`) — **static and
   slot-based**, set once at host creation via `POST /v1/admin/hosts`
   ([`db/host.rs:80-108`](../../../../valori-ui/backend/apps/api/src/db/host.rs)), incremented by `WorkerService::reserve_slot`
   after each successful deploy ([`worker_service.rs:92-94`](../../../../valori-ui/backend/apps/api/src/worker_service.rs)), never
   derived from actual CPU/RAM/disk. This is the only capacity number
   `placement::place()` reads ([`placement.rs:35-64`](../../../../valori-ui/backend/apps/api/src/provision/placement.rs)).
2. **Reported capacity** (`cpu_pct, memory_pct, disk_pct, container_count,
   reported_project_count, reported_available_slots`) — would be
   **dynamically reported** by a worker's heartbeat if one existed
   ([`main.rs:1448-1466`](../../../../valori-ui/backend/apps/api/src/main.rs)), stored ([`db/host.rs:120-158`](../../../../valori-ui/backend/apps/api/src/db/host.rs)), but
   **never read by the placement engine** — comment explicitly says a
   mismatch between reported and authoritative numbers is left visible
   rather than auto-reconciled ([`db/host.rs:99-105`](../../../../valori-ui/backend/apps/api/src/db/host.rs)).

Since no worker agent exists, #2 is entirely inert today — every host's
`cpu_pct`/`memory_pct`/etc. is `NULL` unless an operator has manually
backfilled it. Placement decisions are made purely on the **static,
operator-declared `capacity_slots`** — "how many project-slots I said this
box has," not any measured resource. This is the number a future placement
engine would need to make real if it wants resource-aware scheduling; today
it's a manually-set integer.

---

## Existing Bootstrap

**FACT** — `bootstrap-valori-control-plane.sh` ([header](../../../../valori-ui/backend/scripts/bootstrap-valori-control-plane.sh)) hardens a
fresh Azure Ubuntu 24.04 VM into a Docker-ready **control-plane** host. Its
own header states: *"This script ONLY bootstraps the HOST. It does not
deploy any application code... or open any application port."* It is not
worker-specific and installs nothing worker-agent-related (there is nothing
to install — the agent doesn't exist).

`harden-ssh.sh` is SSH hardening, orthogonal to worker bootstrap.

**FACT** — No `worker` bootstrap script, no Docker-install-for-worker
script, no systemd unit template, no worker config template, and no
registration-config template exist in either repository. A grep for
`2375`/`2376`/`dockerd` TLS setup across `backend/scripts` and `docs`
returns nothing — exposing the Docker Engine API on a worker host is
entirely undocumented and unscripted today.

---

## Missing Components

To take `valori-worker-01` from "pinged successfully" to "real Valori
consumer worker," every one of these is currently absent:

1. **Docker installed and running** on the worker VM (or an equivalent
   `Provisioner` target — none exists for anything else).
2. **Docker Engine API reachable** from 172.16.0.4 to 172.16.0.5 on a port
   the control plane will use (`docker_host` column, or the `:2375`
   default) — not configured, not scripted, no TLS guidance implemented.
3. **A `worker_class` decision and region tag** for this host, consistent
   with what a project's plan will request (`worker_class` defaults
   `'standard'`).
4. **An `infra.hosts` row** for 172.16.0.5, created via `POST
   /v1/admin/hosts` (admin-key-gated) — does not exist until an operator
   creates it.
5. **`PROVISIONER=docker` (or `=dokploy`) set on the control plane** —
   currently defaults to `Mock` if unset ([`config.rs:229`](../../../../valori-ui/backend/apps/api/src/config.rs)); if the
   live control plane is still running with `PROVISIONER=mock`, every
   "provision" call today only ever fabricates fake state and touches
   nothing real regardless of what hosts exist in `infra.hosts`.
6. **Caddy (or equivalent) installed on the worker + a DNS zone** for the
   public per-project routing, if public reachability is wanted (not
   required for a private-VNet-only PoC).
7. **A worker-heartbeat agent** — does not exist anywhere in either repo.
   Placement doesn't require it (health/heartbeat is not a scheduling
   gate), but without it the host will sit permanently at
   `WorkerHealth::Unknown` and no real CPU/RAM/disk telemetry will ever
   flow.
8. **Object store configuration** (`NODE_OBJECT_STORE_BUCKET` on the
   control plane) if durability beyond local container-volume storage is
   required — currently optional/absent by default.
9. **Verification against a live host** — `docker.rs` and `caddy_router.rs`
   both explicitly self-describe as "structurally correct, never run for
   real." The very first live deploy attempt is the first real test of this
   code path.

---

## Shared Worker Readiness

The isolation model (container-per-project, described above) is
*architecturally* capable of Project A + Project B sharing one worker.
Nothing in the code artificially blocks it — `placement::place()` will
happily return the same host for two different projects if it has spare
`capacity_slots`, and each gets its own container/port/volume.

What is **not yet proven**:
- The Docker Engine API path has never been run against a real host
  (explicit caveat in `docker.rs`).
- No test in either repo exercises two live containers from two different
  projects coexisting on one Docker host and confirms actual data isolation
  (only unit tests of URL/env/name construction exist, [`docker.rs:547-764`](../../../../valori-ui/backend/apps/api/src/provision/docker.rs)).
- Resource contention is entirely unmanaged today: `capacity_slots` is an
  operator-declared integer with no relationship to the host's real CPU/RAM
  — two "slots" worth of projects could still starve each other on a small
  VM, since heartbeat-reported real usage is never consulted by placement.
- `RuntimeLimits.memory_mb`/`cpu_millis` ARE wired into the container create
  call when a plan sets them ([`docker.rs:406-407`](../../../../valori-ui/backend/apps/api/src/provision/docker.rs)) — so per-container
  resource caps *can* be enforced if the plan configures them; whether the
  worker's real capacity was sized against those caps is an operator
  decision, not something code verifies.

---

## Enterprise Worker Readiness

For a *dedicated* worker (Enterprise tier, presumably one host reserved for
one project or one org), the same container-per-project mechanism already
gives dedicated-host semantics as soon as `worker_class` is used to steer
placement — `placement::place()`'s exact-class-match rule
([`placement.rs:27-34`](../../../../valori-ui/backend/apps/api/src/provision/placement.rs)) already exists specifically so a premium/dedicated
project never lands on shared standard hardware "because it had room." What
is missing is **operational, not architectural**: no `worker_class` tag
scheme has been decided/documented for "dedicated to org X" (as opposed to
"premium hardware tier"), and there is no code path that reserves 100% of
a host's `capacity_slots` for a single org/project (that would need to be
enforced by setting `capacity_slots = replication_count` at host-creation
time, an operator convention, not a system guarantee).

---

## Recommended Implementation

In dependency order:

1. Decide `PROVISIONER=docker` vs `dokploy` for this environment (Docker
   is more mechanically complete in this codebase — no undocumented
   port-exposure guesswork like `dokploy.rs` carries).
2. Install Docker on `valori-worker-01`, expose its Engine API to
   172.16.0.4 only (private VNet, TLS strongly preferred over the
   unauthenticated `:2375` default — `docker_host` column supports an
   `https://` value).
3. `POST /v1/admin/hosts` to register the row (region, worker_class,
   provider, ip=172.16.0.5, docker_host, capacity_slots).
4. Confirm the control plane process actually has `PROVISIONER=docker`
   (not `mock`) — check via the startup log line at [`main.rs:220`](../../../../valori-ui/backend/apps/api/src/main.rs)
   or [`main.rs:460`](../../../../valori-ui/backend/apps/api/src/main.rs) (`provisioner = provisioner_kind_label`).
5. Provision a real test project and observe: does `provision_project_inner`
   actually reach the worker's Docker API and start a container? This is
   the first live verification of `docker.rs` ever performed.
6. Confirm `/health` on the resulting node responds, and a `search`/
   `insert` round-trip against it works.
7. Only after step 6 is proven does building a worker heartbeat agent (a
   small process on the worker calling `POST /v1/internal/heartbeat` every
   ~30s with real `sysinfo`-sourced cpu/mem/disk) become worth doing — it's
   pure observability, not a blocker for provisioning to work.
8. Decide and document the shared-vs-dedicated `worker_class`/`capacity_
   slots` convention before onboarding a second project onto this host.

---

## Exact Next Steps

1. `docker.rs`'s live-host verification (step 5 above) — this determines
   whether the entire Docker provisioning path actually works, not just
   compiles.
2. Decide `docker_host` TLS posture for `valori-worker-01` before exposing
   the Engine API even on the private VNet.
3. Register the host via `POST /v1/admin/hosts` (requires the admin key).
4. Confirm the running control-plane process's actual `PROVISIONER` env
   var — this audit could not read the live process's environment; source
   only shows the default is `Mock`.
5. If durability is required, set `NODE_OBJECT_STORE_BUCKET` on the
   control plane before the first real project provision.
6. Defer the worker-heartbeat-agent build until after step 1 succeeds live.

---

## FINAL VERDICT

```
CONSUMER WORKER:
NOT READY

WORKER AGENT:
MISSING

REGISTRATION:
PARTIAL   (real admin-driven DB-write mechanism exists; no self-registration, no worker-side caller)

HEARTBEAT:
PARTIAL   (real, tested, authenticated server-side endpoint exists; zero worker-side callers anywhere)

PROJECT PROVISIONING:
PARTIAL   (real, code-complete Docker Engine API path exists and is wired into the request flow; explicitly self-documented as never run against a live host, and defaults to Mock unless PROVISIONER is set)

SHARED WORKER:
NOT READY

ENTERPRISE DEDICATED WORKER:
NOT READY

NEXT IMPLEMENTATION PHASE:
Live-verify DockerProvisioner against valori-worker-01 (register host, set PROVISIONER=docker, provision one real test project, confirm the container starts and /health responds) — before building any worker heartbeat agent.
```

STOP.
