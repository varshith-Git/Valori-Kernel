# G2.3.1-B — Caddy Routing Failure After Successful Docker Provisioning

Source-code audit only. **No code changed, nothing implemented, nothing
touched on Azure.** Labels: **FACT** (observed in source, cited),
**OBSERVATION** (pattern across files), **HYPOTHESIS** (plausible,
unprovable from code alone), **DECISION** (this audit's judgment call).

---

## 1. Failure evidence

FACT, as reported and consistent with source (not independently
re-verified against the live worker — no infra access in this session):

- Project `6d88266a-47f6-42bd-a358-58ea0ae6e557` provisioned via
  `DockerProvisioner` against `valori-worker-01` (172.16.0.5, Docker API at
  `2375`, reachable from the control plane at 172.16.0.4).
- Container `valori-6d88266a-47f6-42bd-a358-58ea0ae6e557-0`
  (`ghcr.io/varshith-git/valori-kernel/valori-node:latest`) created,
  started, and reported `healthy`.
- Provisioning ended in `error` because a POST to
  `http://172.16.0.5:2019/config/apps/http/servers/srv0/routes` failed
  ("error sending request for url").
- The container currently publishes `0.0.0.0:32768 -> 3000/tcp`.

```
DockerProvisioner   PASS
Valori Node         PASS
Node health         PASS
CaddyRouter         FAIL
Project provisioning FAIL
```

---

## 2. Actual provisioning sequence (traced from source, not assumed)

```
POST /v1/projects/:id/provision           (main.rs, provision_project)
  -> provision_project_inner
    -> check_quota_and_entitlements
    -> resolve_worker_auth_token
    -> WorkerService::find_available -> placement::place
    -> [optional VolumeService.create]
    -> state.provisioner.deploy(host, &DeployRequest{...})   <-- DockerProvisioner::deploy()
         inside deploy() (docker.rs:346-455), IN THIS ORDER:
           1. pull image (best-effort — a pull error is logged, not fatal)
           2. ensure/mount volume
           3. POST /containers/create        <- container created here
           4. POST /containers/{id}/start    <- container started here
           5. GET /containers/{id}/json (inspect for the assigned host port)
           6. if node_index == 0 && publish:
                self.caddy.add_route(host, project_id, nodes_domain, http_port).await?
                                                    ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
                                                    THIS is what failed
           7. Ok(DeployedNode { host_id, container_id, http_port, raft_port: None })
    -> db::instance::insert(...)             <-- NEVER REACHED (see §8)
    -> WorkerService::reserve_slot(...)       <-- NEVER REACHED
    -> node_url = format!("https://{id}.{nodes_domain}")   <-- NEVER COMPUTED
  -> [on Err] supabase.set_project_status(id, "error")
```

**FACT, confirmed by reading `docker.rs:451` against `docker.rs:454`**: the
Caddy route registration (`self.caddy.add_route(...).await?`) is the
*second-to-last* statement inside `deploy()`, strictly before
`Ok(DeployedNode {...})` is ever constructed. Because it uses `?`, a
failure here makes `deploy()` itself return `Err`, and `?` at the
`provision_project_inner` call site propagates that immediately —
`db::instance::insert` is *never called*. This single ordering fact is the
root cause of §8's orphan-container finding.

---

## 3. Caddy architecture — what the code actually expects

**Answering the 17 numbered questions, in order, each with its source:**

**1. Why does `CaddyRouter` expect `172.16.0.5:2019`?**
`CaddyRouter::admin_url()` (`caddy_router.rs:96-100`) builds the URL as
`http://{host.ip}:{admin_port}` — `host.ip` is `infra.hosts.ip`, i.e. the
value registered for this worker, `172.16.0.5`. `admin_port` defaults to
`2019` (Caddy's own default admin port), hardcoded in `CaddyRouter::
Default` (`caddy_router.rs:64-68`) with no per-host override column.

**2/3. Where is Caddy supposed to run? On every worker?**
On the **same host as the Docker containers** — `add_route`'s upstream
dial is `format!("localhost:{upstream_port}")` (`caddy_router.rs:127`).
This only works if Caddy and the just-created container are on the same
machine, resolving `localhost` to each other. **Yes, by design, Caddy is
expected to run on every provisioning worker** (`Caddyfile.example`'s own
header: *"runs on EVERY provisioning host (a machine registered in
`infra.hosts`...), NOT on the control plane's own droplet"*).

**4/5. Is there already a worker Caddy config/template? Is `host-caddy/`
for this exact host?**
`backend/deploy/host-caddy/` contains **exactly one file**:
`Caddyfile.example`. No `docker-compose.yml`, no install script, nothing
else. Its own header states: *"UNVERIFIED — there is no VPS to run this
against yet."* **Classification: C — stale/unverified, never instantiated
against any real host, including this one.** It is the intended template
for `valori-worker-01`'s role, but nothing has ever deployed it there.

**6. What process should listen on `172.16.0.5:2019`?**
Caddy itself (its built-in admin API), if and when it is installed and
configured to bind there.

**7. What routes should Caddy create?**
One reverse-proxy route per project, `{project_id}.{nodes_domain} ->
localhost:{container_host_port}`, added via `POST /config/apps/http/
servers/srv0/routes` and removed via `DELETE /id/project-{project_id}`
(`caddy_router.rs:115-175`).

**8. How does `node_url` get generated?**
`main.rs:947-948`, only after every host in the deploy loop succeeds:
`format!("https://{id}.{nodes_domain}")` when `uses_dns_routing` is true
(`main.rs:246`: true for both `docker` and `dokploy` provisioners). This
project never reached that line — its `node_url` was never set.

**9. Is the node supposed to be directly exposed on a random Docker port?**
**No** — the random Docker-assigned port is an internal implementation
detail. The *intended* public entry point is Caddy on `443`/`80`, reverse-
proxying to that internal port over `localhost`. The random port itself
was never meant to be customer-facing.

**10. Can `DockerProvisioner` bind the host port to `127.0.0.1`/private
IP only?**
**Yes, trivially** — `PortBinding { host_ip: "0.0.0.0", host_port: "" }`
(`docker.rs:395`) is a plain struct field sent verbatim to Docker's
`/containers/create` API. `host_ip` is not derived from anything else in
the request; changing the literal `"0.0.0.0"` to `"127.0.0.1"` (or
`host.ip`, the worker's private IP) is a one-line, mechanically simple
change — not a redesign. See §4 for why `127.0.0.1` is the better target
given Caddy's own dial address is literally `localhost`.

**11. How is customer traffic supposed to reach the worker?**
Directly — `nodes_domain`'s DNS (`*.nodes.valori.systems`, per
`Caddyfile.example`) is expected to resolve to the worker's **public** IP,
terminating TLS at Caddy `:443` there. **Not proxied through the control
plane.** (This is an external DNS-zone configuration, not verifiable from
source — flagged as an open question, not confirmed either way for
`valori-worker-01` specifically.)

**12. Does the control plane proxy customer traffic, or does DNS point at
the worker?**
DNS points at the worker (see §11) — the control plane's own role in the
data path is provisioning/admin only, per the architecture already
documented in `CLAUDE.md`'s control-plane vs. data-plane split.

**13/14/15. NSG / UFW / never-public ports** — see §5's table.

**16/17. Is worker Caddy required for shared/Enterprise workers?**
**Yes, unconditionally, as currently coded** — `add_route`/`node_url`
generation has no branch for "no Caddy" or "different routing for shared
vs. dedicated hosts." Every `docker`/`dokploy`-provisioned project, on any
worker, shared or dedicated, goes through the exact same Caddy call. There
is no separate mechanism for either tier today.

---

## 4. Real, unresolved contradiction in the existing design

**FACT** — `caddy_router.rs`'s own module doc comment (lines 35-43)
instructs: *"[Caddy's admin API] must never be reachable from the public
internet — bind it to a private network/VPN between the control plane and
each host (or an SSH tunnel), never `0.0.0.0`."* This reads as endorsing a
bind reachable **over the private network** (i.e., the worker's private IP,
`172.16.0.5`) — consistent with how `CaddyRouter::admin_url()` actually
calls it (`http://172.16.0.5:2019/...`).

But `Caddyfile.example` (the one and only template that exists for this
exact purpose) configures:

```
{
	admin 127.0.0.1:2019
}
```

**`127.0.0.1` is loopback-only — unreachable from *any* other host,
including the control plane over the private VNet.** If Caddy were
installed today using this example file verbatim, the control plane's
request to `172.16.0.5:2019` would fail for exactly the reason it already
did — **whether or not Caddy is even running**, this bind address would
still refuse that connection.

**Both files carry their own "never run against a real instance" caveat**
(`caddy_router.rs:2-3`, `Caddyfile.example:7-9`) — this contradiction was
never caught because nothing had exercised this path until the real
attempt that prompted this audit. It is not a redesign to resolve: it's
picking one of the two already-stated intents (network-reachable, matching
`caddy_router.rs`'s actual behavior) and making the template consistent
with the code that calls it — the same private-IP-bind + firewall-allowlist
pattern already used for Docker's own API in the G2.3.1 bootstrap script.

---

## 5. Network topology — actual, not assumed

| Port | What | Classification | Evidence |
|---|---|---|---|
| `22` | SSH | **PRIVATE** — restricted to the operator's IP via UFW | `bootstrap-valori-worker.sh` PHASE 4 |
| `2375` | Docker Engine API | **PRIVATE-VNET** — bound to `172.16.0.5` only, UFW-restricted to `172.16.0.4/32` | `bootstrap-valori-worker.sh` PHASE 6/7; confirmed reachable from the control plane per this session's real test |
| `2019` | Caddy admin API | **INTENDED PRIVATE-VNET** per `caddy_router.rs`'s comment, but **the only existing template binds it LOCALHOST-ONLY** (contradiction — §4). **Currently: nothing listens here at all** — Caddy was never installed by any script in this repo, including my own `bootstrap-valori-worker.sh` (deliberately out of that script's scope). |
| `80`/`443` | Caddy public HTTP/HTTPS | **INTENDED PUBLIC** (customer traffic terminates here, per §3 Q11/Q12) — **not yet opened anywhere**; no UFW rule, no NSG rule confirmed, no Caddy installed to listen on them |
| `3000` (container-internal) | valori-node's own listen port | **LOCALHOST-ONLY by design intent** — Caddy is meant to be the only thing that ever dials it (`localhost:{port}`) |
| dynamic host port (e.g. `32768`) | Docker's published mapping of `3000/tcp` | **CURRENTLY BOUND TO `0.0.0.0`** — §6 below. UFW's default-deny-incoming policy (set by `bootstrap-valori-worker.sh` PHASE 4) blocks it at the host firewall *unless* an operator added a broader rule since. **The Azure NSG layer is independent and was not, and cannot be, verified from this session** — a NSG rule permitting a wider range would bypass UFW's protection entirely. This is stated as unknown, not as safe. |

**Must never be publicly exposed, per the code's own stated intent:**
`2375` (Docker API — unauthenticated, full host control),
`2019` (Caddy admin API — unauthenticated, full Caddy control).

---

## 6. Public node port — why, and what it should be

**FACT** — `docker.rs:394-395`:
```rust
let mut port_bindings = HashMap::new();
port_bindings.insert(CONTAINER_PORT.to_string(), vec![PortBinding { host_ip: "0.0.0.0", host_port: "" }]);
```
`host_ip: "0.0.0.0"` tells Docker to publish the port on **every network
interface** on the host, including any public one. `host_port: ""` tells
Docker to auto-assign a free ephemeral port (hence `32768`) — this part is
intentional and documented (`docker.rs:24-27`'s module doc: *"Docker picks
a free host port itself"*), but the interface it binds is not discussed
anywhere in that comment.

**Given the worker has a real public IP (`20.41.234.189`) confirmed
earlier in this engagement**, `0.0.0.0` binding is a genuine exposure risk
— the only thing standing between the internet and this container's raw,
unauthenticated-by-Caddy HTTP port is UFW's default-deny (host-level) and
whatever the Azure NSG currently allows (unverified, see §5).

**Intended fix target (not implemented here):** `127.0.0.1`, not the
private IP — Caddy's own reverse-proxy dial target is literally
`localhost:{port}` (`caddy_router.rs:127`), so binding the container port
to loopback-only satisfies the *only* consumer the architecture defines,
with zero network exposure, on the private VNet or otherwise. Binding to
`172.16.0.5` (private IP) instead would still work for Caddy (loopback and
the private interface are both locally reachable) but is strictly weaker
than `127.0.0.1` for no benefit, since nothing other than same-host Caddy
is ever meant to reach this port directly.

---

## 7. Failure / rollback behavior

Answering directly:

- **Is the healthy container left behind intentionally? No.** It is an
  unintended, fully untracked orphan — a direct consequence of §2's
  ordering: Caddy registration happens *inside* `deploy()`, before the
  `DeployedNode` result (and therefore the `infra.instances` row) exists.
- **Is there rollback cleanup for this specific failure? No.** Every
  existing cleanup path (`Provisioner::destroy`, the port-collision retry
  at `main.rs:920-933`, disaster-recovery rebuild) operates by looking up
  `infra.instances` rows and calling `destroy(host, container_id,
  project_id)` — none of them can find a container that was never
  recorded. The container is discoverable only via a direct `docker ps`/
  `docker inspect` on the worker itself, matched by its deterministic name
  (`valori-{project_id}-{node_index}`) or its `valori.project_id` label.
- **Should an orphaned container be cleaned automatically? Not decided
  here** — this audit reports the gap; whether the fix is "don't fail
  `deploy()` on a Caddy error" vs. "decouple route registration from
  container creation and clean up on failure" vs. "add explicit orphan
  cleanup on the error path" is a real design decision with tradeoffs
  (see §9's REQUIRED NOW / FUTURE split) — not made unilaterally here.
- **Why did project provisioning mark `error` after node health passed?**
  Because `deploy()`'s contract is all-or-nothing — a route-registration
  failure is treated exactly the same as a container-creation failure by
  the caller, even though the container itself is fine. `mark_project_
  active` (`main.rs:955`) is reached only after every step in the loop
  succeeds; nothing partially succeeds in this model.
- **Is the lack of a Caddy health gate a known design gap? Yes, and it
  compounds this one.** Separately (found in the G2.3.1 audit, unrelated
  to Caddy): nothing in `provision_project_inner` calls the deployed
  node's own `GET /health` before considering it ready, either. Both gaps
  point at the same underlying issue — `deploy()`'s success criterion is
  "the HTTP calls I made all returned 2xx," not "the thing I built is
  actually usable end-to-end."

---

## 8. Minimum required changes (NOT implemented — audit only)

### REQUIRED NOW (to make the existing worker actually provision-able)

1. **Install and run Caddy on `valori-worker-01`.** Nothing does this
   today — not `bootstrap-valori-worker.sh` (deliberately out of that
   script's scope, per its own header), not any other script in this
   repo. Needs: Caddy binary/container + a real (not `.example`) Caddyfile
   derived from `host-caddy/Caddyfile.example` + a DNS provider API token
   for the DNS-01 challenge (Cloudflare, per the example — confirm this is
   actually `nodes.valori.systems`'s real DNS provider before reusing that
   module).
2. **Fix the admin-bind contradiction (§4).** Change `Caddyfile.example`'s
   `admin 127.0.0.1:2019` to bind somewhere the control plane can actually
   reach — `admin 172.16.0.5:2019` (the worker's private IP), paired with
   a UFW rule restricting `2019` to `172.16.0.4/32` only, exactly mirroring
   the existing Docker-API pattern already established for this same
   worker in the G2.3.1 bootstrap script. This is applying an existing,
   already-verified pattern, not inventing a new one.
3. **Confirm `*.nodes.valori.systems` DNS actually resolves to this
   worker's public IP** — an external DNS-zone configuration step, not
   code; unverifiable from this repo.
4. **Bind the container's published port to `127.0.0.1`, not `0.0.0.0`**
   (§6) — a one-line change to `PortBinding.host_ip` in `docker.rs`, closes
   a real (if UFW-mitigated-for-now) exposure.

### NICE TO HAVE

5. Decouple Caddy route registration from container creation inside
   `deploy()`, so a route failure doesn't silently orphan an otherwise-
   healthy container — e.g., return the container as provisioned-but-
   unpublished, let the caller register the route as a separate step it
   can retry/roll back independently (the `publish()` method already
   exists on the `Provisioner` trait for exactly this kind of decoupled
   call — it's currently only used for blue/green cutover, not the
   first-deploy path).
6. Add explicit orphan-container detection/cleanup for exactly this
   failure mode (find-by-label, not just by `infra.instances`).
7. Gate `mark_project_active` on the node's own `GET /health` actually
   responding (the pre-existing gap noted in §7, not new to this audit).

### FUTURE

8. Per-host Caddy admin port override (`infra.hosts.caddy_admin_port` or
   similar) if any worker ever needs a non-default port.
9. A real multi-worker DNS routing story — `nodes_domain` is currently one
   shared value across every host in `DockerProvisioner`; a wildcard DNS
   record can only point at one place, so a second real worker would need
   its own subdomain/DNS strategy, not covered by the current single-
   Caddyfile-per-domain design. Out of scope for making the *first* worker
   functional, but real for the second one.

### Exact files likely to change (when this is implemented, not now)

```
backend/deploy/host-caddy/Caddyfile.example   — fix admin bind address (§4), rename/copy to a real (non-.example) config for actual deployment
backend/scripts/bootstrap-valori-worker.sh    — add a Caddy install phase, or a separate dedicated script
backend/apps/api/src/provision/docker.rs      — PortBinding.host_ip: "0.0.0.0" -> "127.0.0.1" (§6); optionally decouple add_route (NICE TO HAVE #5)
```

Azure-side (not repo files): NSG rule for `2019` restricted to
`172.16.0.4/32`; NSG rules for `80`/`443` if customer traffic is meant to
reach this worker publicly; DNS zone record for `*.nodes.valori.systems`.

---

## 9. Manual verification plan (for whoever implements this later)

1. Confirm `nodes.valori.systems`'s actual DNS provider (must match
   whichever `xcaddy`-built DNS plugin the real Caddyfile uses).
2. Install/run Caddy on the worker with the corrected (private-IP-bound)
   admin config.
3. `curl -m 5 http://172.16.0.5:2019/config/ -H "..."` from the control
   plane — expect a real config JSON back, not a timeout/refused.
4. Re-attempt provisioning the same or a fresh test project; confirm the
   route-add call succeeds and `node_url` gets set.
5. From outside the VNet, confirm `2019` and `2375` are NOT reachable, and
   that the assigned Docker host port is not reachable directly (only
   through Caddy's `443`).
6. Confirm the orphaned container from the failed real attempt
   (`valori-6d88266a-...-0`) is found and either adopted (a manual
   `infra.instances` row inserted, if the operator wants to keep it) or
   destroyed — it is not tracked anywhere today and needs a deliberate
   decision, not an automatic script guess.

---

## Stop conditions check

None triggered. This fix does not require: Docker TLS/mTLS changes (Caddy
admin API auth is a separate, existing, no-code-change concern — bind
address + firewall, same pattern already used for Docker), redesigning
provisioning (the `deploy()` control-flow ordering issue in §7/§9 NICE TO
HAVE is flagged, not required to unblock the REQUIRED NOW list), changing
Project/Collection semantics, a worker agent, a networking redesign (this
is completing the *existing* documented topology, not inventing a new
one), a new cloud provider, or any customer API contract change.

---

## FINAL VERDICT

```
DOCKER PROVISIONING:
PASS

NODE STARTUP:
PASS

NODE HEALTH:
PASS

CADDY ROUTING:
FAIL — Caddy was never installed on valori-worker-01 by any script in this repo, and the one existing config template (host-caddy/Caddyfile.example) binds its admin API to 127.0.0.1, which would still refuse the control plane's request even if Caddy were installed as-is — a real, previously-untested contradiction against how caddy_router.rs actually calls it.

PROJECT READINESS:
FAIL — provisioning correctly refused to mark the project active given the Caddy failure, but left the underlying container as an untracked orphan (no infra.instances row was ever created, since route registration happens before that insert inside deploy()).

NETWORK SECURITY:
FAIL — the container's published port is bound to 0.0.0.0 (all interfaces) rather than 127.0.0.1; the worker has a real public IP; UFW's default-deny is the only currently-confirmed mitigation, and the Azure NSG layer could not be verified from this session.

NEXT PHASE:
Install and correctly configure Caddy on valori-worker-01 (private-IP-bound admin API + UFW-restricted to the control plane, mirroring the existing Docker-API pattern), fix the container port binding to 127.0.0.1, and manually resolve the orphaned container from this real attempt — then re-run provisioning end-to-end.
```

STOP.
