# Phase G2.3.2 — Real Worker Caddy + Provisioning Cleanup

## Goal

Close the gap found in `docs/reviews/worker-g2.3.1-caddy-routing-audit.md`:
make Caddy actually work on `valori-worker-01` so a project can reach
`READY`, and stop provisioning failures from orphaning healthy-but-
untracked containers.

## What was and wasn't possible in this session

Same constraint as every prior phase in this engagement: **no SSH/Azure
access.** I did the code-level work fully (implemented, compiled, tested
against real local mock servers) and produced the operational artifacts an
operator needs (a corrected Caddyfile, a Caddy install script). I did
**not** install anything on the real worker, did not touch any Azure NSG,
and did not re-run provisioning against real infrastructure. Every claim
below says explicitly which category it's in.

---

## 1. Worker architecture (confirmed, not re-derived)

```
Project
  → DockerProvisioner (control plane, 172.16.0.4)
    → Docker Engine API (worker, 172.16.0.5:2375, private-VNet-only)
      → valori-node container (published to 127.0.0.1:<dynamic>, as of this phase — see §5)
        → Caddy (worker-local, admin API on 172.16.0.5:2019, private-VNet-only)
          → reverse-proxies *.nodes.valori.systems to localhost:<dynamic>
            → node_url (customer-facing, https://{project_id}.nodes.valori.systems)
```

## 2. Caddy installation (deliverable, not executed)

**Delivered:**
- [`backend/deploy/host-caddy/Caddyfile`](../../../valori-ui/backend/deploy/host-caddy/Caddyfile)
  — the real config, superseding `Caddyfile.example` (kept, now labeled
  "superseded, historical reference only"). The one concrete fix: `admin
  {$CADDY_ADMIN_BIND}` (env-substituted, set to the worker's own private
  IP by the install script) instead of the example's hardcoded `admin
  127.0.0.1:2019` — that hardcoded loopback bind is what the real audit
  proved unreachable from the control plane.
- [`backend/scripts/bootstrap-worker-caddy.sh`](../../../valori-ui/backend/scripts/bootstrap-worker-caddy.sh)
  — downloads a custom Caddy build (DNS-01 plugin, via Caddy's own
  build-server API, no local Go/xcaddy toolchain needed), installs it as a
  systemd service, deploys the real Caddyfile with the corrected admin
  bind, restricts `2019` to `CONTROL_PLANE_CIDR` via UFW (same pattern as
  the Docker-API port in `bootstrap-valori-worker.sh`), leaves `80`/`443`
  closed by default.
- README section documenting both.

**Decision made, not pre-specified in the repo:** Caddy runs **natively**
(systemd service), not in Docker. `caddy_router.rs`'s upstream dial is
literally `localhost:{port}` — running Caddy in a container would need
`--network host` to make that resolve correctly, a broader blast-radius
than a normal systemd service scoped by regular OS permissions, for no
benefit. Documented as a decision, not asserted as the only valid choice.

**Not independently confirmed:** whether `nodes.valori.systems` is
actually on Cloudflare (the DNS-01 provider both `Caddyfile.example` and
this phase's real `Caddyfile` assume) — this is an external DNS-zone fact,
unverifiable from source. The script refuses to run without
`CLOUDFLARE_API_TOKEN` (or an explicit different `DNS_PROVIDER`), and its
header calls this out explicitly rather than silently assuming.

**NOT PERFORMED — no infrastructure access:** actually running this script
on `valori-worker-01`.

## 3. Caddy security

Per the audit: `2019` restricted to `CONTROL_PLANE_CIDR` only, via UFW —
implemented in `bootstrap-worker-caddy.sh` PHASE 5, same rule shape as the
Docker API port. `80`/`443` are **opt-in** (`OPEN_PUBLIC_HTTP=true`),
default closed.

**NOT PERFORMED — no infrastructure access:** the Azure NSG rule (`allow
TCP 2019, source 172.16.0.4/32, destination 172.16.0.5/32`) needed
alongside UFW — I cannot create or verify Azure NSG rules from this
session. External verification ("from public internet: 2019 → blocked")
also NOT PERFORMED for the same reason. **I am not claiming these are
verified** — see §9's exact commands for you to run.

## 4. Node port binding — fixed

**Delivered (code, tested):** `PortBinding.host_ip` in
[`docker.rs`](../../../valori-ui/backend/apps/api/src/provision/docker.rs)
changed from `"0.0.0.0"` to `"127.0.0.1"`. This is the narrowest bind that
still satisfies Caddy's own dial target (`localhost:{port}`) — nothing
else is ever meant to reach this port directly, on the private VNet or
otherwise. Confirmed no other code path reads `DeployedNode.http_port` to
build a customer-facing URL for the docker provisioner (`uses_dns_routing
= true` for docker, so the `http://{host.ip}:{http_port}` fallback in
`main.rs` is Mock-only and never exercised for a real docker deploy).

**NOT PERFORMED — no infrastructure access:** confirming this against the
real worker (the existing orphaned container from the real attempt was
created before this fix, still bound to `0.0.0.0` — see §7).

## 5. Caddy routing (traced again, unchanged from the audit)

Confirmed, no new findings beyond the prior audit: route path
`{project_id}.{nodes_domain} -> localhost:{port}`, registered via `POST
/config/apps/http/servers/srv0/routes` with a stable `@id: project-{uuid}`
tag, removed via `DELETE /id/project-{uuid}`. HTTPS is expected
(automatic, via the wildcard cert Caddy's own DNS-01 challenge maintains).
No config reload needed — Caddy's admin API mutates the live config
directly. This part of `caddy_router.rs` was **not modified** — the
contract it expects is unchanged; only the previously-missing
infrastructure it depends on (§2) and the failure handling around it (§6)
changed.

## 6. Orphan-container cleanup — implemented and tested

**Root cause** (from the audit, restated): `DockerProvisioner::deploy()`
called `caddy.add_route(...).await?` — and the two other Docker-lifecycle
calls after container creation (`start`, port-`inspect`) — as bare `?`
propagations, with `Ok(DeployedNode {...})` only reachable after all of
them succeed. A failure in any of the three left a live container with no
`infra.instances` row, invisible to every other cleanup path in the
codebase.

**Fix, delivered and tested:** a new `cleanup_orphaned_container()` method
on `DockerProvisioner`, called from all three post-creation failure
points (start failure, port-inspect failure, Caddy route failure — the
brief named Caddy specifically, but the same bug existed at the other two
points too, so the fix covers all three, not just the one that happened to
be hit first). It:
1. Calls `destroy_container()` (the container is force-removed).
2. If that succeeds **and** this deploy created its own volume
   (`req.data_volume.is_none()` — the same rule `destroy()` already uses
   to avoid touching a `VolumeService`-owned volume), deletes that volume
   too.
3. Logs the outcome — `info` on success, `error` (distinct level) if the
   container removal itself fails, explicitly calling out that manual
   removal is then required.
4. **Never returns an error itself** — the caller always gets back the
   *original* provisioning error (start/inspect/Caddy failure), never a
   cleanup-related one. Proven directly by
   `cleanup_failure_does_not_hide_the_original_caddy_error` (§8).

## 7. Project state — no change needed, confirmed by re-reading

Per the brief's instruction to implement "only the minimum gate required
by the actual existing architecture": **nothing needed changing here.**
`mark_project_active` (`main.rs`) was already, before this phase,
reachable only after every step in `provision_project_inner`'s deploy loop
succeeds — which already includes the Caddy route registration (it's
inside `deploy()`, per §1). The cleanup fix in §6 doesn't touch this gate;
it only makes the failure path leave less debris behind. No new project
state was invented.

**One gate explicitly NOT added, and why:** "node is healthy" is listed in
the brief's Part 6 as a desired gate, but nothing in the *existing*
architecture checks the node's own `GET /health` before proceeding — this
is a real, separately-tracked gap already flagged in the prior G2.3.1
audit, not something this phase's IN-SCOPE list asked for ("gate on
health" isn't one of the 14 enumerated in-scope items). Adding it now
would be inventing a new gate beyond what "the actual existing
architecture" requires, which the brief explicitly says not to do. Left
as a known gap, not silently added.

## 8. Tests — real, run, passing

All in [`docker.rs`](../../../valori-ui/backend/apps/api/src/provision/docker.rs),
against an in-process mock Docker Engine API (an axum server — the same
framework this crate already uses for its real API, not a new mocking
dependency):

| Test | Proves |
|---|---|
| `caddy_route_failure_cleans_up_the_orphaned_container_and_volume_and_returns_the_original_error` | (1) Caddy failure after container creation triggers cleanup; (2) the original `ProvisionError::DnsRouting` is what's returned |
| `cleanup_failure_does_not_hide_the_original_caddy_error` | (3) a cleanup failure (mocked container-delete 500) still surfaces the original Caddy error, never a cleanup-related one; volume cleanup is correctly skipped once container cleanup itself failed |
| `successful_provisioning_does_not_delete_the_node_or_its_volume` | (4) a fully successful deploy never touches the container/volume it just created |
| all 16 pre-existing `docker.rs` tests | (5) existing `DockerProvisioner` behavior (env construction, volume naming, URL construction, unreachable-worker handling) is unchanged |

`CaddyRouter`'s admin port (`2019`) is a hardcoded private field with no
test-injection seam — these tests rely on `127.0.0.1:2019` being either
closed (the two failure tests) or bindable by the test itself (the success
test, which skips gracefully, not fails, if something else already owns
that port locally). A `static Mutex` serializes all three so they don't
race each other over that one shared fixed port under Rust's default
parallel test execution — verified deterministic across repeated local
runs (see below), not asserted from a single pass.

```
$ cargo test -p valori-cloud-api docker::
running 19 tests
... (16 pre-existing, all ok)
test provision::docker::tests::caddy_route_failure_cleans_up_the_orphaned_container_and_volume_and_returns_the_original_error ... ok
test provision::docker::tests::cleanup_failure_does_not_hide_the_original_caddy_error ... ok
test provision::docker::tests::successful_provisioning_does_not_delete_the_node_or_its_volume ... ok
test result: ok. 19 passed; 0 failed

$ cargo test -p valori-cloud-api
test result: ok. 110 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
```
Re-run 3× locally to check for the parallel-port race — stable all three
times.

## 9. Real Azure verification — NOT PERFORMED

Every item in the brief's Part 8/9/11 (worker inspection, re-running the
real project, external port probing) requires SSH/Azure access this
session does not have. Exact commands for you to run, in order:

```bash
# 1. Copy the corrected artifacts to the worker
scp backend/deploy/host-caddy/Caddyfile valoriadmin@20.41.234.189:/tmp/
scp backend/scripts/bootstrap-worker-caddy.sh valoriadmin@20.41.234.189:/tmp/

ssh valoriadmin@20.41.234.189

# 2. Inspect the orphan BEFORE touching it (per the brief's Part 9 instruction)
docker inspect valori-6d88266a-47f6-42bd-a358-58ea0ae6e557-0
docker ps -a --filter "name=valori-6d88266a"
# This container predates the port-binding fix (§4) — it is still bound to
# 0.0.0.0, and no cleanup code ran for it (it was created before this
# phase existed). It will NOT be auto-removed by anything — the new
# cleanup code only runs during a NEW deploy() call's failure path, not
# retroactively. Decide manually: keep it (if you want to adopt it — would
# need a manual `infra.instances` row) or remove it:
#   docker rm -f valori-6d88266a-47f6-42bd-a358-58ea0ae6e557-0

# 3. Deploy the control-plane binary with this phase's DockerProvisioner
#    changes (127.0.0.1 bind + cleanup fix) — however you normally deploy
#    backend/ (see backend/deploy/docker-compose.yml, `docker compose up
#    -d --force-recreate api` after rebuilding/pulling the new image).

# 4. Install Caddy
mkdir -p /tmp/host-caddy && cp /tmp/Caddyfile /tmp/host-caddy/
sudo CONTROL_PLANE_CIDR="172.16.0.4/32" \
     WORKER_PRIVATE_IP="172.16.0.5" \
     CLOUDFLARE_API_TOKEN="<real token — confirm Cloudflare is actually correct first>" \
  bash /tmp/bootstrap-worker-caddy.sh
# (adjust the Caddyfile path resolution per the script's PHASE 3 comment
# if your copy layout differs from backend/scripts/../deploy/host-caddy/)

# 5. Verify from the worker itself
sudo systemctl status caddy --no-pager
curl -m 5 http://172.16.0.5:2019/config/
sudo ufw status verbose

# 6. Verify from the control plane
curl -m 5 http://172.16.0.5:2019/config/    # must succeed

# 7. Verify from outside the VNet (a machine that is NOT the control plane)
curl -m 5 http://20.41.234.189:2019/config/   # must time out / refuse
curl -m 5 http://20.41.234.189:2375/version   # must time out / refuse

# 8. Re-provision the SAME project (per the brief's Part 9 — do not create a new one)
curl -X POST https://api.valori.systems/v1/projects/6d88266a-47f6-42bd-a358-58ea0ae6e557/provision \
  -H "Authorization: Bearer <session token>" -H "content-type: application/json" \
  -d '{"region":"southindia","replication":1,"dim":768,"index":"brute"}'

# 9. Confirm project state, node_url, and a real insert/search/delete —
#    same commands as the G2.3.1-B runbook already gave you.

# 10. Confirm no orphan remains after a deliberate FAILURE test — e.g.
#     temporarily block 2019 with `sudo ufw delete allow ...`, attempt a
#     second provision of a throwaway test project, confirm the container
#     this phase's cleanup code removed it automatically, then restore the
#     UFW rule.
```

---

## Files changed

```
backend/apps/api/src/provision/docker.rs   — 127.0.0.1 port bind; cleanup_orphaned_container(); 3 new tests; module doc comment corrected (Docker verified, Caddy gap documented)
backend/deploy/host-caddy/Caddyfile        — NEW: the real, admin-bind-corrected config
backend/deploy/host-caddy/Caddyfile.example — annotated "superseded", kept as historical reference
backend/scripts/bootstrap-worker-caddy.sh  — NEW: Caddy install/config/firewall script
backend/scripts/README.md                  — new section documenting the above
```

## Infrastructure changed

**None.** No Azure resource, VM, NSG rule, or DNS record was created or
modified — this session has no credentials or network path to make any of
those changes.

## Rollback procedure (for whoever runs §9)

- Caddy: `sudo systemctl disable --now caddy` — the worker reverts to
  exactly its post-G2.3.1 state (Docker only, no Caddy). Idempotent to
  rerun `bootstrap-worker-caddy.sh` afterward.
- Port binding: if `127.0.0.1` binding breaks something unexpected, the
  one-line revert is `docker.rs`'s `PortBinding { host_ip: "127.0.0.1", ...
  }` back to `"0.0.0.0"` — no data loss either way, this only affects
  newly-created containers, not existing ones.
- Orphan cleanup: purely additive failure-path behavior — no rollback
  concern; a successful deploy's behavior is provably unchanged (§8's third
  test).

---

## Stop conditions check

None triggered. Specifically: Caddy's intended architecture WAS
determinable from source (§1-§5); `CaddyRouter` needed no new API contract
(only its already-existing calls needed a reachable target); `node_url`
semantics are unchanged; secure port binding was achievable with a
one-line change (§4); cleanup required no new state machine (§7 — the
existing gate already worked); no existing project data was touched;
no public node port is required (§4's `127.0.0.1` bind is the narrowest
valid option, satisfying the existing architecture's own dial target).

---

## FINAL VERDICT

```
DOCKER PROVISIONING:
PASS — unchanged from the real G2.3.1-B attempt; code changes in this phase don't touch this path's success behavior (proven by the unchanged pre-existing tests + the new success-path test)

NODE STARTUP:
PASS — same reasoning

NODE HEALTH:
PASS — real evidence from the G2.3.1-B attempt (Docker's own HEALTHCHECK reported healthy); not re-verified live in this phase (no infra access), and NOT gated by deploy() today (§7 — a known, separately-tracked, out-of-scope gap)

CADDY:
NOT PERFORMED — installation script delivered and code-reviewed, never run against the real worker

CADDY ROUTING:
NOT PERFORMED — same reason; the code-level contract (§5) is unchanged and was never the broken part

PROJECT READY:
NOT PERFORMED — depends on the above

NODE API / INSERT / SEARCH / DELETE:
NOT PERFORMED — no live node reachable through Caddy to test against

ORPHAN CLEANUP:
PASS (code-level) — 3 new tests, all passing, deterministic across repeated runs; NOT PERFORMED against the real orphaned container (§9 step 2 — needs a manual decision, not an automatic script)

PUBLIC NODE PORT EXPOSURE:
PASS (code-level) — 0.0.0.0 → 127.0.0.1 fix implemented and reasoned through; NOT PERFORMED — not verified against the real (still-0.0.0.0-bound, pre-fix) orphaned container or any newly-created one

SECURITY:
NOT PERFORMED — UFW rule is scripted (bootstrap-worker-caddy.sh PHASE 5) but never applied to the real worker; Azure NSG rule not created or verified; external port-blocked verification not performed

FILES CHANGED:
backend/apps/api/src/provision/docker.rs
backend/deploy/host-caddy/Caddyfile (new)
backend/deploy/host-caddy/Caddyfile.example (annotated only)
backend/scripts/bootstrap-worker-caddy.sh (new)
backend/scripts/README.md

INFRASTRUCTURE CHANGED:
none

REMAINING BLOCKERS:
1. No SSH/Azure access in this session — every real-worker step (§9) needs a human operator with that access.
2. Real DNS provider for nodes.valori.systems still unconfirmed (assumed Cloudflare, matching the pre-existing example file — never independently verified).
3. The pre-existing orphaned container (valori-6d88266a-...-0) needs a manual operator decision (adopt or remove) — the new cleanup code does not retroactively touch it.

NEXT PHASE:
A human operator runs bootstrap-worker-caddy.sh on valori-worker-01 (§9), confirms Azure NSG allows 172.16.0.4/32 -> 2019, decides the orphaned container's fate, and re-provisions project 6d88266a-47f6-42bd-a358-58ea0ae6e557 end-to-end — the first real test of this phase's code against real infrastructure.
```

STOP.
