# Caddy 2.11.4 Route-Array Insert Semantics — Source-Verified

Investigation only. **No live Caddy config was mutated in this session**
(this session has no infrastructure access at all — every live command in
this thread was run by the operator, not by me). **No Rust code was
changed.** This corrects a wrong claim from the prior phase's own
"CONFIDENCE: HIGH" answer, which turned out to be exactly backwards.

---

## What actually happened, live

```
1. DELETE /config/apps/http/servers/srv0/id/project-0ce442c9-... → 200, route gone
2. POST   /config/apps/http/servers/srv0/routes/0 (exact saved route body) → 200
3. GET    /config/apps/http/servers/srv0/routes → route at index 1, not 0
```

The `200` on step 2 was real, and the write genuinely happened — it just
didn't land where the previous phase assumed it would.

## Source-level evidence (Caddy `v2.11.4`, `admin.go`, `unsyncedConfigAccess`)

Fetched directly from `github.com/caddyserver/caddy` at the `v2.11.4` tag
— not recalled from memory, not inferred from public docs (the prior
phase's mistake). The function that handles a `/config/[path]` request
whose final path segment is a numeric array index has one switch
statement per HTTP method:

```go
case http.MethodPost:
    if ellipses {
        valArray, ok := val.([]any)
        if !ok {
            return fmt.Errorf("final element is not an array")
        }
        v[part] = append(arr, valArray...)
    } else {
        v[part] = append(arr, val)
    }
```
**POST appends to the end of the array.** The numeric index in the URL
path is not used to position the inserted value — it's parsed and
bounds-checked, but the actual mutation is a plain `append`.

```go
case http.MethodPut:
    arr = append(arr, nil)
    copy(arr[idx+1:], arr[idx:])
    arr[idx] = val
    v[part] = arr
```
**PUT is the shift-insert operation** — it grows the array by one,
shifts every element from `idx` onward one position to the right, then
writes the new value into the now-vacated `idx` slot. This is the actual
"insert at position N" primitive Caddy implements.

```go
case http.MethodPatch:
    arr[idx] = val   // direct assignment, no shift — replaces in place
```
PATCH exists too, but overwrites the element at `idx` rather than
inserting — using it at index 0 would destroy the wildcard fallback, not
relocate the project route.

```go
case http.MethodDelete:
    v[part] = append(arr[:idx], arr[idx+1:]...)
```
DELETE removes element `idx`, shifting left — this part of the prior
phase's understanding (and the code's existing `remove_route()`) was
already correct.

Bounds check (paraphrased from the same function): `idx < 0`, or `idx >
len(arr)`, is always an error; `idx == len(arr)` is allowed **only** for
`PUT` (append-via-insert-at-end is a valid PUT target, not a valid GET/
POST/PATCH/DELETE target at that index). Irrelevant to our case since the
target index is always `0`, always in bounds.

## Why the live behavior matches exactly

Starting array before step 2: `[fallback]` (the project route was already
removed by step 1). `POST /routes/0` — per the source above — ignores the
`0` and appends: result `[fallback, project]`. Project lands at index 1.
**Exactly what was observed.** There is no normalization, no reordering,
no hidden Caddy behavior beyond this — the `200` response was truthful
about the append succeeding; it was never going to insert at the front,
regardless of what index was named in the URL.

## Why the duplicate-`@id` `400` from the previous audit turn is *also*
fully consistent with this (not a separate mystery)

That earlier attempt used a bare `POST /config/apps/http/servers/srv0/
routes` (no trailing index at all, or an index that's still handled by the
identical append-only POST case above) against an array that still had the
OLD project route present (the delete-first step wasn't done, or wasn't
done successfully, before that particular attempt) — append-with-existing-
duplicate-`@id` is exactly what produces Caddy's config-indexing validation
error at the two positions the duplicate ends up occupying. Same POST
semantics, same root behavior, two different symptoms depending on whether
a stale duplicate was already present.

## Alternative approaches considered (per the brief's list) — ruled in/out with evidence

| Option | Verdict |
|---|---|
| A. POST to `/routes` with a special payload | No such special payload/query-param exists in the source read above — POST's behavior is unconditional append (with one exception: a body that is itself an array *and* the path ends in `...`, which unpacks/merges multiple elements — still always appends, never positions) |
| B. POST then a second operation | Unnecessary — PUT alone does the correct insert-at-index in one call |
| **C. `PUT /routes/0`** | **This is the correct, source-confirmed operation** |
| D. DELETE + POST with a specific payload form | Ruled out — no payload shape changes POST's append-only behavior |
| E. PATCH-like operation | PATCH exists but replaces in place (destroys whatever was at that index) — wrong tool for insert |
| F. A different config endpoint entirely | Not needed — the same `/config/apps/http/servers/srv0/routes/{idx}` path, just with the PUT verb, is correct |
| G. Route sorting/ordering outside the JSON array | No such mechanism found in the source read |
| H. Caddyfile ordering + dynamic subroute insertion | Out of scope — would mean redesigning the Caddyfile's route structure, not a targeted fix |

## Is the G2.3.2-F implementation actually correct?

**No.** `CaddyRouter::add_route()` (`backend/apps/api/src/provision/
caddy_router.rs`) issues `self.http.post(self.admin_url(host, "/config/
apps/http/servers/srv0/routes/0"))`. Per the source evidence above, this
is functionally **identical to the original bug** — both the pre-fix bare
`POST .../routes` and the "fixed" `POST .../routes/0` append to the end of
the array. **The G2.3.2-F fix did not fix the bug.** It happened to pass
its own local tests because the mock Caddy server built for those tests
(an axum router I wrote) implemented `POST /routes/0` as "insert at that
index" — an assumption carried over from the same wrong mental model, not
from real Caddy's behavior. The tests were internally consistent with a
false premise; they never had a way to catch this, because nothing in that
mock was checked against real Caddy source until this investigation.

## Exact Rust code change required (not implemented in this session)

One-line change, `caddy_router.rs`, inside `add_route()`:
```rust
// current (wrong — appends, does not insert):
let resp = self
    .http
    .post(self.admin_url(host, "/config/apps/http/servers/srv0/routes/0"))
    .json(&route)
    .send()
    .await
    ...

// required:
let resp = self
    .http
    .put(self.admin_url(host, "/config/apps/http/servers/srv0/routes/0"))
    .json(&route)
    .send()
    .await
    ...
```
`reqwest::Client::put(...)` in place of `.post(...)` — everything else in
the function (the delete-first cleanup, the JSON body, the error handling)
is already correct and untouched.

## Exact test changes required (not implemented in this session)

The 4 tests added in G2.3.2-F (`fallback_exists_before_any_project_route_
is_added`, `adding_a_project_route_inserts_it_before_the_fallback_and_
keeps_the_fallback_terminal`, `a_second_project_route_also_inserts_before_
the_fallback`, `adding_routes_never_moves_the_fallback_off_the_end`) all
assert on the *outcome* (final array order), not the HTTP verb used to get
there — so they remain valid tests of the desired behavior. What must
change is the **mock Caddy router** they run against
(`mock_caddy_router()` in `caddy_router.rs`'s test module): its route
currently registered as
```rust
.route("/config/apps/http/servers/srv0/routes/0", axum_post(...))
```
must become
```rust
.route("/config/apps/http/servers/srv0/routes/0", axum_put(...))
```
(handler body unchanged — `routes.insert(0, route)` was already correct
insert-semantics; only the HTTP method it's bound to was wrong). The same
correction applies to `docker.rs`'s own mock Caddy server in
`successful_provisioning_does_not_delete_the_node_or_its_volume`. Once
both mocks are switched from `axum_post` to `axum_put` on that specific
route, and the Rust `add_route()` fix above is applied, all 4 ordering
tests plus the 3 docker.rs orphan-cleanup tests should continue passing —
**not verified in this session**, since implementing was explicitly out of
scope for this investigation turn.

**A genuinely new risk this correction introduces, worth flagging
explicitly**: the existing mocks were never validated against real Caddy
source before now, and neither will the corrected ones be, by construction
— they're still hand-written fakes, just built from a fact (PUT inserts)
instead of a false one (POST inserts). The only way to fully close this
gap is the live verification steps below, against the real worker — no
amount of additional mock-based unit testing can substitute for that,
which is exactly what this whole investigation thread has been
demonstrating.

## Rollback-safe operator procedure (for whoever repairs the live project route)

Same delete-then-insert shape as before, **with the corrected verb**:
```bash
# 1. Remove the existing (currently-at-index-1) route
curl -X DELETE http://172.16.0.5:2019/id/project-0ce442c9-96c9-4884-b62f-db816ad90ac5

# 2. Confirm removal
curl http://172.16.0.5:2019/config/apps/http/servers/srv0/routes
# expect: exactly 1 element (the fallback)

# 3. Insert at index 0 — PUT, not POST
curl -X PUT http://172.16.0.5:2019/config/apps/http/servers/srv0/routes/0 \
  -H "Content-Type: application/json" \
  -d '<exact saved route body, from /tmp/caddy-routes-before-repair.json or step 1's own captured response>'

# 4. Verify
curl http://172.16.0.5:2019/config/apps/http/servers/srv0/routes
# expect: project route at index 0, fallback (terminal:true, untouched) at index 1
```
Rollback (restore the original, pre-repair — buggy but known — ordering),
same correction applied:
```bash
curl -X DELETE http://172.16.0.5:2019/id/project-0ce442c9-96c9-4884-b62f-db816ad90ac5
curl -X PUT http://172.16.0.5:2019/config/apps/http/servers/srv0/routes/1 \
  -H "Content-Type: application/json" \
  -d '<route body from /tmp/caddy-routes-before-repair.json>'
```

**Do not run any of the above from this session — no infrastructure
access exists here, and the task explicitly asked for investigation only.
This is written for the operator.**

---

## FINAL VERDICT

```
LIVE POST /routes/0 SEMANTICS:
Appends to the end of the array. The trailing numeric index in the URL path is parsed and bounds-checked but is NOT used to position the inserted value for the POST method — confirmed directly from Caddy v2.11.4's admin.go source (unsyncedConfigAccess), not inferred.

ROOT CAUSE:
The G2.3.2-F fix used the wrong HTTP verb. POST and PUT's roles on a numeric array-index path are the reverse of what was assumed: POST always appends; PUT inserts-and-shifts at the given index. The fix changed the URL path (adding /0) but kept the POST verb, which made zero functional difference from the original bug — both are appends.

CORRECT OPERATION:
PUT /config/apps/http/servers/srv0/routes/0 (delete-by-@id first, exactly as add_route() already does, then PUT instead of POST to insert at the front)

RUST CODE STATUS:
NEEDS CHANGE — caddy_router.rs's add_route() must use .put(...) instead of .post(...) on the indexed route-insert call; both test-module mock Caddy servers (caddy_router.rs and docker.rs) must register that path under PUT, not POST, for their assertions to mean anything against real Caddy behavior.
```

Not implemented in this session, per this task's investigation-only scope. Ready to implement on request.

STOP.
