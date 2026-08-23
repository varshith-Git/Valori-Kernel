# Phase G2.3.2-F — Caddy Route Order Bug

## Goal

Real live evidence (confirmed against the actual worker's Caddy admin API,
not a hypothesis) showed the terminal wildcard `404` fallback route
evaluated *before* a project's own route, making every project
permanently unreachable through Caddy regardless of the route itself
being correctly registered. Fix the insertion order.

## Root cause

`CaddyRouter::add_route()` (`backend/apps/api/src/provision/
caddy_router.rs`) registered every new project route with a plain
```
POST /config/apps/http/servers/srv0/routes
```
— Caddy's admin API appends a plain POST to the **end** of the array.
`host-caddy/Caddyfile`'s `*.nodes.valori.systems` site block ends in a
`terminal: true` `respond 404` fallback, which was already the sole/first
entry (loaded from the static Caddyfile at Caddy startup) before any
project route was ever added. Caddy's HTTP server evaluates `routes`
strictly in array order and stops at the first **matching** terminal
route — it does not prefer a more specific host match over array
position. Since the fallback matches every `*.nodes.valori.systems`
hostname (including every real project's own subdomain), and every
project route was appended *after* it, the fallback always won first.
Confirmed live: index 0 = fallback (terminal), index 1 = the project's own
route — exactly matching the reported symptom.

## Fix

One-line change, `caddy_router.rs`'s `add_route()`:
```
POST /config/apps/http/servers/srv0/routes/0
```
Caddy's admin API supports POST-to-an-indexed-array-path as documented,
existing insertion semantics (caddyserver.com/docs/api) — this inserts at
position 0, pushing every existing entry (the fallback, and any other
project's route already registered) down by one. Since every add always
targets index 0, the fallback's position monotonically increases with
each addition — it ends up at the **highest** index (genuinely last),
not merely "before the newest one." The public API of `CaddyRouter` is
unchanged; nothing about Caddy's config model was redesigned; the
fallback was neither deleted nor made non-terminal — it remains exactly
what it was, just correctly ordered relative to it.

`publish()` (the blue/green cutover path, which also calls `add_route`
internally... actually calls `self.caddy.add_route` directly in
`docker.rs`) automatically inherits the same fix — there is only one
`add_route` implementation.

## Existing project repair

Project `0ce442c9-96c9-4884-b62f-db816ad90ac5` already has its route
sitting at index 1, behind the fallback, from before this fix existed.
**Not repaired in this session** — no infrastructure access, same
limitation as every real-Azure/production step in this engagement. The
brief's own preferred approach (re-run `add_route` for this exact project,
which now naturally reinserts it at index 0 ahead of the fallback) is the
correct minimal repair — no manual Caddy admin API surgery needed beyond
calling the already-fixed code path again. Exact command for whoever has
real access:
```bash
# From the control plane, re-registers this project's route in the
# now-correct position — does NOT recreate the container, does NOT touch
# the database (add_route is a pure Caddy config call)
curl -X POST http://172.16.0.5:2019/config/apps/http/servers/srv0/routes/0 \
  -H "Content-Type: application/json" \
  -d '{
    "@id": "project-0ce442c9-96c9-4884-b62f-db816ad90ac5",
    "match": [{"host": ["0ce442c9-96c9-4884-b62f-db816ad90ac5.nodes.valori.systems"]}],
    "handle": [{"handler": "reverse_proxy", "upstreams": [{"dial": "localhost:32770"}]}]
  }'
# Best practice per add_route()'s own logic: DELETE the stale one first
# (idempotent — 404 if it's not there) so there's never a duplicate @id:
curl -X DELETE http://172.16.0.5:2019/id/project-0ce442c9-96c9-4884-b62f-db816ad90ac5
# then the POST above.
```
(Port `32770` is the value from the reported live evidence — confirm it's
still the container's actual published port before reusing it; if the
container was ever restarted since, Docker may have reassigned it.)

## Tests

Added to `caddy_router.rs`, against a stateful in-process mock of Caddy's
admin API (axum — already this crate's own framework, no new test
dependency) that tracks a real ordered array and implements exactly the
two operations `add_route`/`remove_route` use: POST-to-indexed-path
inserts at that position, DELETE `/id/:id` removes wherever an entry
currently is.

| Test | Proves |
|---|---|
| `fallback_exists_before_any_project_route_is_added` | (1) the seeded fallback is present |
| `adding_a_project_route_inserts_it_before_the_fallback_and_keeps_the_fallback_terminal` | (2) the fallback is untouched/still `terminal: true`; (3) the new route's index is less than the fallback's |
| `a_second_project_route_also_inserts_before_the_fallback` | (6) a second project's route also lands ahead of the fallback |
| `adding_routes_never_moves_the_fallback_off_the_end` | (7) across 3 successive additions, the fallback's index is always `len - 1` — never anywhere but last |

**Tests 4 and 5 from the brief** ("exact project hostname reaches project
route" / "unmatched hostname reaches fallback") **were not implemented as
Rust unit tests** — proving that requires Caddy's own host-matching
*engine* to actually run and decide which route wins for a given request,
which the mock (a plain ordered-array tracker) does not, and should not,
re-implement — doing so would mean reimplementing Caddy itself, fragile
and out of scope. That end-to-end proof needs a real Caddy binary; see
"Real verification" below for the exact commands. The four implemented
tests fully prove the actual bug's fix (ordering), which is the part that
was wrong.

`CaddyRouter::admin_port` remains a hardcoded private field with no test
seam — same constraint as the G2.3.2 Docker tests. These new tests bind
the real fixed `127.0.0.1:2019` port and share the exact same lock the
Docker tests use (`CADDY_PORT_TEST_LOCK`, moved to `provision/mod.rs` in
this phase — see below) — Rust runs tests in parallel by default, and this
port is process-wide OS state shared across **both** files, not per-file,
so a per-file lock would not have prevented cross-file races.

## Incidental fixes required to make the tests actually pass and be clean

1. **`docker.rs`'s own pre-existing success-path test** (`successful_
   provisioning_does_not_delete_the_node_or_its_volume`, from Phase
   G2.3.2) had its own separate mock Caddy server registered only at the
   old, non-indexed `/routes` path. Once `add_route()` started POSTing to
   `/routes/0`, that test's mock stopped matching and the test failed
   (caught immediately by rerunning the full suite, not shipped
   unnoticed) — fixed by updating that mock's route to `/routes/0`, no
   change to what the test actually verifies.
2. **`CADDY_PORT_TEST_LOCK` moved from a file-local `docker::tests`
   static to a shared `pub(crate)` static in `provision/mod.rs`.** The new
   `caddy_router.rs` tests bind the exact same fixed port; a lock scoped
   to only `docker.rs`'s test module could not protect against
   `caddy_router.rs`'s tests running concurrently against the same OS
   port. Confirmed necessary empirically, not just in theory — omitting
   this reproduced the exact class of cross-file port race the original
   per-file lock was already built to prevent within one file.
3. **The lock itself changed type**, `std::sync::Mutex` → `tokio::sync::
   Mutex`. `cargo clippy` correctly flagged `await_holding_lock` — every
   test holds this guard across multiple `.await` points (binding the
   mock listener, driving `add_route`), which a std-sync `MutexGuard` is
   not intended to survive. Tokio's async-aware mutex is the correct tool
   for exactly this "hold across awaits" shape; switching removed all 7
   instances of the warning with no behavior change (no poisoning to
   recover from either, so `.lock().await` replaces `.lock().unwrap_or_
   else(|e| e.into_inner())` directly).

## Explicitly deferred, per the brief

**Docker host-port exposure** (`0.0.0.0:32770 -> 3000`, or in the current
code post-G2.3.2, `127.0.0.1:<port>`) — a separate hardening concern, not
touched or mixed into this fix, exactly as instructed.

## Verification

```
$ cargo fmt --check          → clean for all 3 changed files (the repo-
                                wide reformat cargo fmt wanted to apply to
                                ~47 unrelated pre-existing files was
                                reverted — not shipped; see "Files
                                changed" below)
$ cargo build -p valori-cloud-api --tests   → clean
$ cargo test -p valori-cloud-api            → 114/114 pass, stable across
                                               3 repeated runs (checked
                                               specifically for the
                                               cross-file port race)
$ cargo clippy -p valori-cloud-api --all-targets --all-features
                                             → 0 warnings in any of the 3
                                               changed files; 5 remaining
                                               warnings are all pre-
                                               existing, in unrelated files
                                               (too-many-arguments x2,
                                               from_* naming x1,
                                               collapsible-if x1,
                                               useless-vec x1), not
                                               touched
```

No frontend files were touched — TypeScript/ESLint/build were not re-run.

**Real production verification: NOT PERFORMED.** Same limitation as every
infrastructure-touching phase in this engagement — no SSH/Azure access in
this session. Exact commands for whoever has real access, in order:

```bash
# 1. Confirm the CURRENT live (still-broken) order on the real worker,
#    matching the brief's own reported evidence:
sudo curl -sS http://172.16.0.5:2019/config/apps/http/servers/srv0/routes | jq '.[]."@id", .[].terminal'

# 2. Deploy the fixed control-plane binary (this phase's caddy_router.rs
#    change) — however the control plane is normally redeployed
#    (docker compose up -d --force-recreate api, per the G2.3.2 doc).

# 3. Repair the EXISTING project's route — see "Existing project repair"
#    above (re-run add_route, or the manual DELETE+POST equivalent).

# 4. Re-inspect the order — expect every project-<uuid> route before the
#    fallback, fallback strictly last:
sudo curl -sS http://172.16.0.5:2019/config/apps/http/servers/srv0/routes | jq '.[]."@id"'

# 5. From the worker itself:
curl -k -i --resolve 0ce442c9-96c9-4884-b62f-db816ad90ac5.nodes.valori.systems:443:127.0.0.1 \
  https://0ce442c9-96c9-4884-b62f-db816ad90ac5.nodes.valori.systems/health
# Expect: HTTP 200, Valori node health JSON — not the Caddy fallback 404.

# 6. From outside (a Mac, or anywhere off the worker):
curl -I https://0ce442c9-96c9-4884-b62f-db816ad90ac5.nodes.valori.systems
# Expect: NOT the Caddy fallback 404 (assuming DNS + the wildcard cert are
# already correctly set up per the earlier DNS/node_url decision phase —
# this fix alone doesn't change either of those).

# 7. Confirm the control-plane scheduler's own /v1/usage poll against this
# project stops reporting it unreachable (whatever log/metric surfaces
# that today).
```

---

## FINAL VERDICT

```
CADDY ROUTE ORDER:
PASS (code-level, 4/4 new tests) — NOT PERFORMED against the real worker

PROJECT ROUTE:
PASS (code-level) — inserted at index 0 on every add_route() call

WILDCARD FALLBACK LAST:
PASS (code-level) — proven for 1, 2, and 3 successive additions; remains terminal:true throughout

EXISTING PROJECT REPAIRED:
NOT PERFORMED — no infrastructure access; exact repair command given above

PUBLIC PROJECT HEALTH:
NOT PERFORMED — real-worker/DNS verification, no infrastructure access

SCHEDULER USAGE:
NOT PERFORMED — same reason

DOCKER PORT HARDENING:
DEFERRED — explicitly out of scope for this phase, per the brief

FILES CHANGED:
backend/apps/api/src/provision/caddy_router.rs
backend/apps/api/src/provision/docker.rs
backend/apps/api/src/provision/mod.rs
```

STOP.
