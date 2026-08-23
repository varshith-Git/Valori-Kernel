# Phase G2.3.1 — Real Azure Consumer Worker

## Goal

Make `valori-worker-01` (172.16.0.5, Azure, same VNet as the control plane
at 172.16.0.4) usable as a real Valori consumer host through the
**existing, unmodified** `DockerProvisioner` path, and prove it end-to-end
with a real project, a real container, and a real vector insert/search —
using only what the prior audit (`docs/reviews/worker-g2.3-consumer-
readiness.md`) established already exists in code.

## Result summary

This phase produced the artifacts an operator needs to complete the real
Azure setup (bootstrap script, a corrected security design, tests), and
surfaced one genuine blocker that changed the security approach mid-phase.
It did **not** reach a real container running on 172.16.0.5, because doing
so requires SSH/administrative access to the Azure VMs and the production
control-plane's admin API key — access this session does not have and
cannot obtain. Every claim below is labeled by how it was actually
verified: **RUN** (executed in this session, real output attached), or
**NOT PERFORMED — no infrastructure access** (requires credentials/network
access unavailable here). Nothing below is asserted as done against real
Azure infrastructure unless it says RUN against a real host, because it
wasn't.

---

## 1. Environment

FACT, unchanged from the prior audit, not independently re-verified here
(no network path to Azure from this session): control plane
`valori-control-plane-01` (172.16.0.4, public `https://api.valori.systems`),
worker `valori-worker-01` (172.16.0.5), both on `vnet-southindia-1` /
`snet-southindia-1`, private ICMP connectivity previously confirmed 3/3.

## 2. Azure networking

**NOT PERFORMED — no infrastructure access.** This session has no Azure
CLI, no NSG-inspection tool, and no network path to either private IP.
What the required topology should be was already documented in the prior
audit's Networking section and is unchanged by this phase's findings,
except for one correction: the Docker API port is `2375` (plain HTTP), not
`2376`/TLS — see §3 and §4 below for why.

## 3. Worker bootstrap

**Delivered:** [`backend/scripts/bootstrap-valori-worker.sh`](../../../valori-ui/backend/scripts/bootstrap-valori-worker.sh)
in `valori-ui`, documented in [`backend/scripts/README.md`](../../../valori-ui/backend/scripts/README.md)
(new "Consumer worker host bootstrap" section). Purpose-built, not a copy
of `bootstrap-valori-control-plane.sh`: installs Docker CE only (no
compose/buildx — a worker only ever receives single-container
`POST /containers/create` calls), configures a Docker TCP listener, and
firewalls it to the control plane's IP only. Idempotent, same
logging/state conventions as the existing control-plane script.
**Syntax-checked (`bash -n`) — RUN, passed.** Never executed against a real
host (no SSH access to `valori-worker-01`).

**Genuine mid-phase finding that changed the design:** the brief's Part 3
asked to "determine exactly how the existing `DockerProvisioner` connects
to Docker" and listed "Docker TCP API with TLS" as a possibility, with
"missing Docker TLS support" as an explicit mandatory-stop condition. On
inspection:

```
backend/apps/api/src/provision/docker.rs:80:   Self { http: reqwest::Client::new(), ... }
backend/apps/api/src/provision/caddy_router.rs:66: Self { http: reqwest::Client::new(), ... }
backend/apps/api/src/provision/dokploy.rs:51:  http: reqwest::Client::new(),
backend/Cargo.toml (workspace): reqwest = { version = "0.12", default-features = false, features = ["json", "rustls-tls"] }
```

All three provisioner-adjacent HTTP clients construct a bare
`reqwest::Client::new()` — no `.add_root_certificate()`, no `.identity()`.
The workspace's `reqwest` build uses `rustls-tls`'s bundled Mozilla root
store, not the OS trust store, and no client certificate is ever
configured. Consequence: **the existing code cannot verify a private/
self-signed CA, and cannot present a client certificate for mutual TLS.**
The only TLS that would actually work is a certificate from a publicly
trusted CA (e.g. Let's Encrypt via a real domain) — not applicable to a
private-IP-only worker without standing up public DNS, which is out of
scope here.

This is exactly the brief's own "missing Docker TLS support" stop
condition. Per the brief's own rule ("DO NOT choose a new mechanism unless
the existing implementation forces that decision" / "If you discover that
one of these is required, STOP and report it instead of implementing it"),
this phase did **not** add TLS support to `DockerProvisioner` — that would
be a change to live provisioning code, unverifiable against real
infrastructure in this session, and explicitly out of scope. Instead the
bootstrap script implements the mechanism the code actually supports and
that `docker.rs`'s own module doc comment already names as acceptable for
a private-network deployment: plain HTTP, Docker's TCP listener bound to
the worker's **private IP only** (never `0.0.0.0`), firewalled via UFW to
accept connections **only** from the control plane's private IP. See the
script's header comment for the full writeup — it's deliberately verbose
there because this is the one design decision in this phase most likely to
be second-guessed later.

## 4. Docker configuration

Per §3: `daemon.json` sets `"hosts": ["unix:///var/run/docker.sock",
"tcp://<worker-private-ip>:2375"]` — the socket is untouched, the TCP
listener binds only the private IP. A systemd override clears Docker's
default `-H` flags (which otherwise conflict with `daemon.json`'s `hosts`
key). UFW then restricts inbound `tcp/2375` to `CONTROL_PLANE_CIDR` (a
required script variable, validated to reject `0.0.0.0/0`). This matches
`docker.rs`'s own documented security note verbatim: *"must never be
reachable from the public internet — bind it to a private network/VPN
between the control plane and each host."*

**NOT PERFORMED — no infrastructure access:** actually running this on
172.16.0.5, confirming the listener binds correctly, and confirming the
firewall rule holds under a real external probe.

## 5. Authentication

Unchanged from the prior audit: per-worker `wtk_...` tokens
(`infra.worker_tokens`) authenticate the `POST /v1/internal/heartbeat`
route only — irrelevant to this phase, since no heartbeat agent exists or
was built here (explicitly out of scope). Docker-API-level authentication
does not exist in the current code (§3) — its protection is network
isolation only, as designed above.

## 6. Worker registration

**Delivered (as instructions, not as an executed call):** the existing,
unmodified endpoint is used —

```
POST /v1/admin/hosts
```

gated by `x-admin-key` (`AdminAuth`), body mapping to `NewHost { region,
worker_class, provider, ip, dokploy_url, docker_host, capacity_slots,
hostname }` (`backend/apps/api/src/db/host.rs:80-90`). No new endpoint was
invented. `docker_host` is left `NULL` so `DockerProvisioner` falls back to
its documented default, `http://{ip}:2375`
(`docker_api_base()`, `docker.rs:86-91`).

**NOT PERFORMED — no infrastructure access.** This requires the production
`ADMIN_API_KEY`, which this session does not have and was not given. The
exact `curl` command an operator needs is in the bootstrap README's
"After it finishes" section.

## 7. Provisioning path

Re-confirmed (not re-derived from scratch — the prior audit already traced
this) against current source, unchanged:

```
POST /v1/project/:id/provision  (AuthUser)
  -> provision_project (main.rs)
    -> provision_project_inner
      -> check_quota_and_entitlements
      -> resolve_worker_auth_token
      -> WorkerService::find_available -> placement::place (status=Active, worker_class exact match, most-free-first)
      -> [optional VolumeService.create — DockerProvisioner requests DockerVolume]
      -> Provisioner::deploy   <-- DockerProvisioner talks straight to the worker's Docker API here
      -> db::instance::insert
      -> WorkerService::reserve_slot
    -> supabase.mark_project_active(id, node_url)   <-- ONLY reached if every deploy() above returned Ok
```

**Genuine, already-true-in-code finding relevant to the brief's Part 11
item 5 ("provisioning failure does not claim project READY")**: confirmed
by reading `main.rs` — `mark_project_active` (which sets
`public.projects.status = 'active'`) is called exactly once, at the very
end of `provision_project_inner`, strictly after the loop over every chosen
host's `provisioner.deploy(...).await?` has returned `Ok`. The `?`
propagates any `Err` out of the function immediately, so a failed `deploy()`
already cannot reach `mark_project_active` — **no code change was needed
for this property; it already holds.** This is proven at the unit level in
§11 below (mocking a live end-to-end failure would require standing up a
fake Supabase + Postgres harness this repo does not currently have — see
Known Limitations).

**Real caveat found, not fixed:** nothing in this chain calls the deployed
node's own `GET /health` before `mark_project_active` runs. Readiness is
inferred purely from the Docker `create`/`start` HTTP calls succeeding, not
from the node process actually answering `/health`. The brief's own goal
chain lists "health check" as a required step between "node startup" and
"project ready." This is a real gap between what the brief asks for and
what current code does — flagged here, **not silently patched**, because
fixing it means changing live provisioning logic (`provision_project_inner`)
that this session cannot test against a real node, and the brief's stop
conditions include "ambiguity about project isolation" / "inability to
verify the real provisioning path" for exactly this kind of situation. See
Next Phase.

## 8. Container deployment

**NOT PERFORMED — no infrastructure access.** `DockerProvisioner::deploy`
was re-read line-by-line (unchanged since the prior audit): pulls the
image, creates/mounts a volume, creates a container named
`valori-{project_id}-{node_index}` with `VALORI_BIND, VALORI_DIM,
VALORI_INDEX, VALORI_MAX_RECORDS, VALORI_SNAPSHOT_PATH=/data/snapshot.bin,
VALORI_EVENT_LOG_PATH=/data/events.log, VALORI_AUTH_TOKEN`, starts it,
reads back the Docker-assigned host port. This session has no way to
execute this against 172.16.0.5's Docker API (no network path, no
credentials) and did not fabricate a run.

## 9. Project setup

**NOT PERFORMED — no infrastructure access.** No project named
`demo-consumer-worker` (or any other) was created against the production
control plane — doing so requires a real `AuthUser` session against
`https://api.valori.systems`, which this session cannot authenticate as.

## 10. Vector test

**NOT PERFORMED — no infrastructure access.** No health/insert/search/
delete/search cycle was run against a real deployed node, because no real
node was deployed (§8/§9).

## 11. Persistence test

**NOT PERFORMED — no infrastructure access,** for the same reason: there is
no live container to restart and re-query.

## 12. Shared-worker smoke test

**NOT PERFORMED — no infrastructure access.** The prior audit's
"Multi-Project Isolation" section already established the isolation
mechanism (container-per-project: distinct container, port, named volume,
`VALORI_DIM`, auth token per project) exists in code and requires no new
backend functionality — the brief's own condition for attempting this
step. This phase did not additionally invent any shared-worker scheduling,
per the explicit prohibition, and did not fabricate a live A/B isolation
proof.

## 13. Tests

**Delivered and RUN** (real `cargo test` output, not summarized from
memory):

Added two tests to
[`backend/apps/api/src/provision/docker.rs`](../../../valori-ui/backend/apps/api/src/provision/docker.rs)
(`deploy_returns_err_not_panic_when_worker_is_unreachable`,
`status_returns_err_not_panic_when_worker_is_unreachable`). No mocking
framework was added (none existed and none was introduced, per the
brief's "do not introduce a new test framework" / "do not add large
integration infrastructure"): each test binds a `TcpListener` to an
ephemeral port and immediately drops it, guaranteeing the OS refuses the
next connection attempt — the local, dependency-free stand-in for "the
Azure worker is unreachable." Each asserts `deploy()`/`status()` return
`Err(ProvisionError::Http { .. })`, never panic, never a fabricated `Ok`.

```
$ cargo test -p valori-cloud-api docker:: --no-fail-fast
running 16 tests
test provision::docker::tests::deploy_returns_err_not_panic_when_worker_is_unreachable ... ok
test provision::docker::tests::status_returns_err_not_panic_when_worker_is_unreachable ... ok
... (14 pre-existing tests, all ok)
test result: ok. 16 passed; 0 failed; 0 ignored; 0 measured; 91 filtered out

$ cargo test -p valori-cloud-api
test result: ok. 107 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
```

**What was NOT added, and why (matches the brief's own scope limits):**
- Brief items 1–3 ("DockerProvisioner points to the expected worker",
  "project provisioning reaches DockerProvisioner", "worker/project
  isolation") are already covered by the pre-existing unit tests in this
  file (`docker_api_base_defaults_to...`, `explicit_docker_host_overrides...`,
  `volume_name_is_stable_per_project_and_node`,
  `self_managed_volume_names_collide_across_a_blue_green_pair`) — nothing
  new was needed.
- Brief item 5 ("provisioning failure does not claim project READY") is
  true by construction in `main.rs` (see §7) but proving it with a live
  integration test would require a Postgres + fake-Supabase-HTTP test
  harness that does not exist anywhere in this crate today (no `tests/`
  integration directory, no HTTP-mocking dev-dependency). Building that
  harness is itself "large integration infrastructure" the brief
  explicitly says not to add. Flagged as a coverage gap, not silently
  built around.
- Brief item 6 ("health check is required before success") could not be
  tested because it isn't true yet — see §7's caveat. A test would either
  have to assert the (missing) behavior against nothing, or assert the
  current (gap) behavior, which would just pin the gap rather than prove
  anything useful.

## 14. Observability

No new observability subsystem was added (explicitly out of scope). The
existing `tracing::warn!` call sites in `docker.rs`/`main.rs` already log
image-pull failures, volume-cleanup failures, and DNS-route-cleanup
failures with context (host, project_id, error) — sufficient for
diagnosing the failure classes in scope here once real deploys are
attempted.

## 15. Failure handling

Of the five cases requested (worker unreachable, Docker unavailable,
invalid credentials, container creation failure, container unhealthy):
**(A) worker unreachable** is the one this phase could actually exercise
without live infrastructure, and it is now covered by §13's tests — the
project cannot be marked READY because `deploy()` returns `Err`, which
`main.rs`'s `?` propagates before `mark_project_active` is ever reached.
**(B) Docker unavailable at the daemon level**, **(C) invalid Docker
credentials** (not meaningfully distinct from (A) given §3's plain-HTTP
design — there are no credentials to be invalid), **(D) container creation
failure**, and **(E) container unhealthy** all require either a real
Docker host or a scripted fake HTTP server returning specific malformed
responses — out of scope for this phase's dependency-free-testing
constraint and not attempted, rather than faked.

## 16. Security verification

**NOT PERFORMED — no infrastructure access** for the live parts (external
probing of `worker:2375`, confirming SSH restriction from outside). What
*can* be stated: the bootstrap script's own design (§3/§4) never
configures a listener on `0.0.0.0`, never opens a port to `0.0.0.0/0`
(both are hard `exit 1` checks in the script for `CONTROL_PLANE_CIDR`),
and the script's PHASE 8 health report explicitly lists what was and
wasn't opened. None of that is a substitute for actually probing the live
host from outside the VNet, which this session cannot do.

---

## Files changed

- **New:** `valori-ui/backend/scripts/bootstrap-valori-worker.sh`
- **New:** two tests + doc comment in `valori-ui/backend/apps/api/src/provision/docker.rs`
- **Modified:** `valori-ui/backend/scripts/README.md` (new "Consumer worker host bootstrap" section)
- **New:** this file, `Valori-Kernel/docs/phases/phase-g2.3.1-real-azure-consumer-worker.md`
- No file in `Valori-Kernel` (the kernel/node crates) was touched — nothing
  in this phase required a kernel-repo change.
- No production control-plane code path (`main.rs`, `worker_service.rs`,
  `placement.rs`, the `Provisioner` trait) was modified.

## Infrastructure changes

**None.** No Azure resource, NSG rule, VM configuration, Docker
installation, or database row was created or modified by this session —
this session has no credentials or network path to make any of those
changes, and did not attempt to work around that.

## Known limitations

1. **This phase does not prove the worker works.** It proves the worker
   *can theoretically be made to work* through the existing code path, and
   ships the exact script + instructions to do so. Nothing here is a
   substitute for actually running the bootstrap script on
   `valori-worker-01` and provisioning a real project.
2. **No TLS / no Docker-API authentication is possible today** without a
   code change to `DockerProvisioner`'s `reqwest::Client` construction —
   see §3. The worker's Docker API is protected by network isolation only.
3. **No health-check gate before `mark_project_active`** — see §7. A
   container that starts but never becomes healthy (crash loop, wrong
   image entrypoint, etc.) would still mark the project "active" today.
4. **No integration-test harness exists** in `backend/apps/api` (no
   `tests/` directory, no Postgres/Supabase test fixtures, no HTTP-mocking
   dev-dependency) — several of the brief's requested tests (provisioning-
   failure-doesn't-mark-ready, end-to-end isolation) cannot be proven
   without either building that harness (out of scope) or real
   infrastructure (unavailable in this session).
5. **`PROVISIONER` runtime value on the live control plane is unknown to
   this session.** The prior audit found the code default is `Mock`; §6/§7
   of this doc's "After it finishes" instructions tell the operator how to
   confirm the running process's actual value — this session cannot check
   it directly.

## Next phase

Live-verify: run `bootstrap-valori-worker.sh` on `valori-worker-01`,
register it via `POST /v1/admin/hosts`, confirm `PROVISIONER=docker` on
the running control-plane process, provision one real test project, and
confirm a container starts and `/health` responds — that is the first
actual test of any of this against real infrastructure, and it is a
human-with-SSH-and-admin-key task, not something this session can execute.
Only after that succeeds does closing the health-check gate (§7) become
worth doing.

---

## FINAL VERDICT

```
CONSUMER WORKER:
FAIL   (not yet attempted live — blocked on infrastructure access this session does not have)

DOCKER PROVISIONER:
FAIL   (unreachable-worker handling verified locally — 2/2 new tests pass; never run against a real Docker host)

PROJECT PROVISIONING:
FAIL   (not exercised live; "does not falsely mark ready on failure" confirmed true by code reading, not by a live test)

NODE HEALTH:
FAIL   (not exercised; also a genuine gap — nothing in the provisioning path currently calls /health before marking a project active)

INSERT:
FAIL   (not attempted — no real node was deployed)

SEARCH:
FAIL   (not attempted — no real node was deployed)

PERSISTENCE:
FAIL   (not attempted — no real node was deployed)

SHARED WORKER:
NOT TESTED

SECURITY:
FAIL   (design corrected mid-phase to match what the existing code can actually do — plain HTTP behind a firewall rule, not TLS — but never verified against a real, externally-probed host)

CODE CHANGES:
valori-ui/backend/apps/api/src/provision/docker.rs (2 new tests + doc comment, no behavior change)

INFRASTRUCTURE CHANGES:
none

REMAINING BLOCKERS:
1. This session has no SSH/administrative access to valori-worker-01 (172.16.0.5) or valori-control-plane-01 (172.16.0.4), and no production ADMIN_API_KEY — every live-verification step (Parts 6-10, 12, 14-15 of the brief) requires a human operator with that access to run bootstrap-valori-worker.sh, register the host, and provision a real test project.
2. DockerProvisioner's reqwest client has no code path for TLS/mTLS against a private CA (see §3) — acceptable for a private-VNet-only PoC (plain HTTP + firewall), but a real limitation if this worker is ever reachable over anything less trusted.
3. No health-check gate exists between container startup and marking a project "active" (see §7) — a real gap against the phase's own stated goal chain, not fixed in this phase because it touches live provisioning logic this session cannot test.

NEXT PHASE:
A human operator runs bootstrap-valori-worker.sh on valori-worker-01, registers it via POST /v1/admin/hosts, confirms PROVISIONER=docker on the live control-plane process, and provisions one real test project — the first actual execution of any of this against real infrastructure.
```

STOP.
