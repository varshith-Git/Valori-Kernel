# Shared Free-tier hosting: live end-to-end verification

Date: 2026-09-12. Commit under test: `5196526` (main, HEAD at verification
start). OS: Darwin 25.6.0 (arm64). Rust: `rustc 1.91.1`, `cargo 1.91.1`.
Docker: `29.6.1` (Docker Desktop, desktop-linux context).

## 1. Executive verdict

```text
PARTIAL — code/local Docker verified, Azure pending
```

The node-level shared-hosting implementation (`crates/valori-node/src/shared.rs`)
is real, correct on every isolation property tested, and passed a full local
Docker verification with two disposable projects on one container. **The
Cloud control-plane integration described in `docs/shared-hosting.md` does
not exist** — no code path anywhere in `valori-ui` (Next.js or the Rust
`backend/apps/api`) ever registers a project on a shared worker. New Free
projects are provisioned exactly like paid projects today: one dedicated
container each. Azure verification was not performed — no credentials were
available in this environment.

## 2. Architecture verified

```text
VERIFIED: main.rs:31-54 — VALORI_SHARED_ROOT present → SharedHost::open(),
  asserts VALORI_CLUSTER_MEMBERS absent, requires VALORI_SHARED_ADMIN_TOKEN,
  VALORI_SHARED_MAX_PROJECTS defaults to 100, binds and serves host.router().
  Absent VALORI_SHARED_ROOT → normal cluster/standalone dispatch, unchanged.

VERIFIED: shared.rs:85-113 (SharedHost::open) — scans every UUID-named
  subdirectory of the root, skips ones with no project.json (interrupted
  creation), rejects any path that escapes the root, deserializes each
  manifest, and calls open_project() for every one — EAGERLY, before
  serving any request. One corrupt project.json or events.log makes
  open_project() return Err, which propagates out of SharedHost::open()
  entirely — the whole worker fails to start. Reproduced directly by the
  existing test shared_registration_is_idempotent_bounded_and_rejects_corruption
  (corrupts one project's events.log, asserts SharedHost::open() is Err).

VERIFIED: shared.rs:197-243 (create_project) — PUT /shared/projects/{id}
  requires the admin bearer token (check_admin), validates token length and
  max_records bounds, is idempotent for an identical re-registration (200),
  returns 409 for a conflicting re-registration or when at capacity, builds
  a fresh directory and calls open_project(), persists the manifest, then
  inserts into the in-memory map.

VERIFIED: shared.rs:53-58, 150-195 (Manifest, open_project) — the manifest
  stores only token_hash (BLAKE3 of the worker token), max_records, and
  active — never the raw token. open_project() gives EACH project its own
  Engine::with_config(...) with its own snapshot_path, event_log_path, and
  AesGcmVault(shred.log) rooted at <shared-root>/<project-id>/ — confirmed
  independent per project, not shared KernelState/namespaces.

VERIFIED: shared.rs:39-45 (authorized) — token check is
  blake3::hash(bearer_token) constant-time-compared against the stored
  hash; used both for admin routes (check_admin) and per-project data
  routes (dispatch), with DIFFERENT expected hashes (host.admin_hash vs.
  project.manifest.token_hash) — an admin token cannot double as a data
  token and vice versa (see Isolation matrix, Credentials row).

VERIFIED: shared.rs:115-126 (SharedHost::router) — the project ID is
  extracted from the path directly via axum's `:id` and `*path` extractors
  on `/shared/projects/:id/node/*path`; NOT inferred from any token.
  dispatch() (shared.rs:293-330) looks up `id` in the registry, THEN checks
  the request's token against that specific project's stored hash. Project
  identity selects the tenant; the token only authorizes it — exactly the
  two-part model documented, not token-only routing.

VERIFIED: shared.rs:293-330 (dispatch) — inner router is the SAME
  build_router() used by standalone nodes (`crate::server::build_router`),
  constructed once per project in open_project() and reused across
  requests via `project.router.clone().oneshot(req)`. A fixed denylist of
  path prefixes (metrics, v1/keys, v1/models/, v1/storage/,
  v1/replication/, internal/, v1/crypto/, v1/records/encrypted,
  v1/snapshot/{save,restore,upload}, non-GET .../index, v1/index/rebuild)
  returns 403 before reaching the inner router. `/v1/proof/event-log` is
  NOT on this denylist — confirmed reachable in Stage 4 evidence below.

VERIFIED: shared.rs:249-265 (set_active) — flips only the `active` flag on
  the manifest and persists it; does not touch the resident Engine. A
  suspended project's dispatch() call returns 409 CONFLICT
  ("project is stopped") without unloading anything from memory — the
  known "stopping doesn't free RAM" limitation is a direct reading of this
  function, not an inference.

VERIFIED: shared.rs:267-291 (delete_project) — removes only that project's
  map entry and directory tree, under the SAME write-lock that also gates
  create/suspend, so it waits for in-flight dispatches to finish before
  deleting files.

VERIFIED: shared.rs:223-228 — admission is `projects.len() >= host.max_projects`,
  a plain count comparison. No memory, CPU, or throughput signal anywhere
  in this function.

VERIFIED: shared.rs:363-503 (export_project / import_project) — export
  requires the project to be inactive first (409 if still active), reads
  events.log + optional metadata/namespace sidecars, computes
  hash_state_blake3(&engine.state) as the expected hash. import_project
  decodes the bundle, replays it into a staging log via
  recover_from_events(), computes the SAME hash function over the replayed
  state, and rejects (400) on any mismatch BEFORE touching the destination's
  live files. This is the only migration direction implemented — the
  destination router in the existing test is a plain build_router()
  (standalone/dedicated), not another SharedHost.

NOT FOUND (control plane): `SHARED_WORKER_URL`, `SHARED_WORKER_ADMIN_TOKEN`,
  `SHARED_WORKER_REGION` — grepped across all of `valori-ui` (both the
  Next.js app and `backend/apps/api`); zero matches anywhere in source.

NOT FOUND (control plane): a `0021_shared_projects.sql` migration, or any
  migration under `supabase/migrations/` or `backend/migrations/` whose
  name or content references shared-worker projects. The migrations that
  exist (`worker_auth_token.sql`, `project_rate_limit_state.sql`, etc.)
  are for the existing per-project dedicated-container model.

NOT FOUND (control plane): a `POST /v1/admin/orgs/{org-id}/dedicate`
  endpoint, or any route/function containing "dedicate" or "shared" (as a
  tenancy concept — two unrelated comment hits on "shared box"/"shared
  process-wide OS state" do not count) in `backend/apps/api/src`.

VERIFIED (control plane, contradicts docs/shared-hosting.md): every new
  project — Free or paid — is provisioned by `provision_project_inner()`
  (`backend/apps/api/src/main.rs:864-933`), which calls
  `check_quota_and_entitlements()` for `RuntimeLimits`, then
  `placement::find_available(&body.region, plan.worker_class(),
  body.replication)`, then `provisioner.deploy()` — the dedicated-container
  `Provisioner` trait (Docker/Dokploy). There is no branch anywhere in this
  function, or in `ui/src/app/dashboard/actions.ts`'s `provisionNewProject()`,
  that calls `PUT /shared/projects/{id}` on any worker. `docs/shared-hosting.md`
  states "The Cloud control plane chooses shared hosting for new free
  projects" — this is not true of the code as checked out; it describes an
  integration that was never built on the Cloud side.

VERIFIED (mechanical compatibility, unexercised in practice): `NodeClient.request()`
  (`valori-ui/ui/src/lib/server/nodeProxy.ts:88-94`) does plain template
  concatenation `` `${this.nodeUrl}${path}` ``, not URL-object parsing that
  would strip a path prefix. If `projects.node_url` were ever populated as
  `http://worker:3000/shared/projects/{id}/node` (the shape
  `docs/shared-hosting.md` specifies), appending `/v1/namespaces` etc.
  would produce exactly the route shared.rs expects. The proxy code is
  compatible with shared hosting today — it has simply never been wired to
  use it, since nothing ever sets `node_url` in that shape.
```

## 3. Test results

| Layer | Test | Result | Evidence |
|---|---|---|---|
| Rust | `cargo test -p valori-node --test shared_hosting -- --nocapture` (pre-existing 5 tests) | PASS | 5/5 passed, 0 failed |
| Rust | Same, after adding 4 new tests (see §7 gap-closing) | PASS | 9/9 passed, 0.36s |
| Rust | `cargo test -p valori-kernel -p valori-node` (full required suite) | PASS | 641 passed, 0 failed, 2 ignored, exit 0 |
| Rust | `cargo fmt --check` | PASS | exit 0 |
| Rust | `cargo clippy -p valori-kernel -p valori-node --all-targets -- -D warnings` | PASS | exit 0, "Finished `dev` profile" |
| Docker | Two projects, one container (this verification, see §5) | PASS | container count 7→8 total (one new: `valori-shvfy-worker-test`); no per-project container created |
| Control plane | Two Free accounts, shared-worker registration | NOT PERFORMED | No code path exists to test (§2, §5) — not a test failure, a missing integration |
| Azure | Live worker | NOT PERFORMED | No Azure credentials or access available in this environment |

No pre-existing failures of any kind were observed in the full suite; no
failure needed reclassification as unrelated.

## 4. Isolation matrix

All rows below are from the real Docker container in §5 unless noted
"(Rust test)" — those are additionally covered by the automated suite so
the property is enforced on every future change, not just this one
manual run.

| Boundary | Result | Evidence |
|---|---|---|
| Credentials | PASS | A→B project 401, B→A project 401, no token 401, data-token→admin route 401, admin-token→data route 401 (§5) |
| Collections | PASS | Both projects created `documents` + `products` with independent record counts; A's collection list contains exactly its own 2 collections (Rust test `shared_projects_isolate_second_collection_with_different_config`) |
| Records | PASS | Record id `0` allocated independently in both projects with different vector values (§5); `GET` returns the correct project-specific value in the pre-existing test |
| Metadata | PASS | Pre-existing test: A's `/v1/memory/meta/set` value never appears in B's `/v1/memory/meta/get` response |
| Graph | PASS | Pre-existing test asserts A's and B's `/v1/graph/nodes` responses differ; §5 confirms graph-node creation succeeds per project |
| WAL | PASS | `<shared-root>/<id>/events.log` is a distinct file per project by construction (`open_project` path join); corrupting one project's log does not corrupt the other's (pre-existing test) |
| State root | PASS | Two mechanisms: (1) restart-boundary equality — pre-existing test; (2) LIVE while both active — new test `shared_live_writes_do_not_cross_project_state_roots` proves a write to A changes only A's `/v1/proof/state`, and symmetrically for B |
| Suspension | PASS | §5: suspending A → 409 on A's `/health`, B's `/health` stays 200 |
| Deletion | PASS | Pre-existing test: deleting A returns 404 on A afterward, B's `/v1/proof/state` is byte-identical to before |
| Restart | PASS | §5: after `docker restart`, both projects' collections, search results, and `final_state_hash` values are byte-identical to their pre-restart values |
| Reactivation | PASS | New test `shared_reactivation_restores_serving_and_preserves_state` + §5: reactivated A serves again with an unchanged state root |

## 5. Container and VM evidence

VM evidence: NOT PERFORMED (local Docker Desktop only; no cloud VM in scope
for this stage).

```text
Containers before this test (docker ps -a):
  cloud-migrate-1
  cloud-postgres-1
  cloud-postgrest-1
  cloud-rest-shim-1
  cloud-ui-1
  cloud-worker-a-1     (pre-existing e2e/cloud stack — standalone dedicated
  cloud-worker-b-1      mode, VALORI_SHARED_ROOT not set; left untouched)
Total: 7

Containers after registering two shared-hosting projects, inserting data,
restarting, suspending, and reactivating:
  (same 7 above, unchanged)
  + valori-shvfy-worker-test   (the ONE new container for this test)
Total: 8

No container named for the test project IDs
(11111111-1111-1111-1111-111111111111 / 22222222-...-222222222222) was
ever created. Both projects' data lived under
/tmp/valori-shvfy-shared-data/<project-id>/ inside the single container's
bind mount.

Project URL shape used (token redacted):
  http://localhost:18300/shared/projects/{project-id}/node/{path}
  Authorization: Bearer <redacted>

final_state_hash values observed (not secret, recorded for the restart
comparison):
  Project A: 2d41d2fa5e38454171d56f194583f6d27960a2d5defe1d9395126a675b20cd5f
  Project B: 7e9f7ab2b483f1b5242b3db454b1a1cf2c36f2a2239386c554e8ad97e88b296f
  (identical before and after `docker restart valori-shvfy-worker-test`)
```

## 6. Failure-path results

| Failure case | Result |
|---|---|
| Named Docker volume with default root ownership, distroless nonroot image | Container started "healthy" but every project-registration call returned 500 (`Permission denied (os error 13)` in container logs) — an environment/deployment finding, not a code defect: `docs/shared-hosting.md` does not document that the shared-root volume must be writable by the image's non-root UID. Worked around for this test with a `chmod 777` bind mount; recorded as a P1 deployment-docs gap in §7/§9, not "fixed" in code. |
| Registering an already-registered project with a different token/limit | Not re-tested live (already covered by the passing `shared_registration_is_idempotent_bounded_and_rejects_corruption` Rust test — 409) |
| Corrupt project log at worker startup | Not re-tested live; covered by the same Rust test, which asserts `SharedHost::open()` returns `Err` |
| Shared worker unavailable during registration | NOT PERFORMED — requires the (nonexistent) control-plane integration to exercise meaningfully |
| Invalid shared-worker admin token | Exercised implicitly: every admin-route call in §5 with a non-admin token returned 401 |
| Shared-worker capacity reached | Not re-tested live; covered by the pre-existing Rust test with `max_projects=1` |
| Control-plane restart during provisioning | NOT PERFORMED — no control-plane provisioning path exists to interrupt |

## 7. Confirmed limitations

| Limitation | Severity | Evidence | Owning phase |
|---|---|---|---|
| Cloud control-plane has zero shared-hosting integration; Free projects always get a dedicated container | **Blocker** — the feature is unusable in production as-is | §2 (NOT FOUND items) | New (not yet in the 7-phase roadmap — a control-plane integration phase, prerequisite to all of Phase 2 onward in `docs/superpowers/specs/2026-09-12-shared-hosting-hardening-design.md`) |
| Shared-root volume must be writable by the image's non-root UID; undocumented | High (deploy-time footgun) | §6 | Deployment docs (`docs/shared-hosting.md`) |
| All project engines loaded eagerly at `SharedHost::open()` | Medium (startup cost scales with tenant count) | §2, confirmed via code read | Phase 4B |
| Suspending a project does not release its engine memory | Medium | §2 (`set_active` code) | Phase 4B |
| No idle eviction | Medium | §2 | Phase 4B |
| Admission is project-count-based, not capacity-aware | Medium | §2 (`shared.rs:223`) | Phase 5 |
| No per-project request-rate protection | Medium | Code read (no rate-limiting code in `shared.rs`) | Phase 3 |
| No per-project usage-entitlement enforcement at the worker | Medium | Code read | Phase 3 |
| No per-project storage-byte enforcement | Medium | Code read | Phase 3 |
| No per-project concurrent-request limits | Medium | Code read | Phase 3 |
| No CPU/RAM hard isolation between projects on one worker | Medium (inherent to the shared-process model) | Documented in `docs/shared-hosting.md:67-69` | Out of scope for a Rust process; accepted tradeoff per the roadmap |
| No on-node GPU/VRAM allocation in shared mode | Low (matches current product surface) | `docs/shared-hosting.md:59-65` | N/A — correctly out of scope |
| One corrupt project log blocks the entire worker's startup | **High** (blast radius = every healthy tenant on that worker) | Confirmed via existing test + code read | Phase 4A (roadmap already marks this a rollout blocker) |
| Shared mode incompatible with Raft | Low (documented, intentional) | `main.rs:32-35` assertion | N/A |
| Some node operations unavailable in shared mode (snapshot admin, key mgmt, replication, on-node ingestion) | Low (documented, intentional) | `shared.rs:311-330` denylist | N/A |
| Pro shared pools not implemented | Medium (product gap, not a bug) | `docs/shared-hosting.md:8-10` | Phase 6 |
| Existing legacy Free projects not automatically migrated | Low (documented, intentional) | `docs/shared-hosting.md:10` | Out of scope |

## 8. Launch recommendation

```text
NO-GO
```

Conditions to reach GO: the node-level implementation itself is solid and
passed every isolation and durability test run against it, live, in Docker.
But the feature does not exist from a user's perspective — no signup flow
anywhere routes a Free project onto a shared worker. Launching "shared
Free hosting" today would launch nothing, because the control plane never
calls it. Required before any GO:

1. Build the missing control-plane integration (SHARED_WORKER_* config,
   the `PUT /shared/projects/{id}` call from `provisionNewProject()` for
   Free-tier projects, `node_url` populated in the
   `http://worker/shared/projects/{id}/node` shape).
2. Fix or document the non-root volume-permission issue found in §6 before
   any real deployment.
3. Land Phase 4A (corrupt-project quarantine) from the hardening roadmap
   before onboarding real users — a single bad tenant currently takes the
   whole worker offline on restart.

## 9. Exact next actions

- **P0** — Design and implement the Cloud→shared-worker integration
  (`backend/apps/api`: env config, a provisioning branch for Free plans,
  `node_url` population; `valori-ui`: no changes needed per §2's proxy
  compatibility finding). Owner: `backend/apps/api/src/provision/`.
- **P0** — Document (and fix, e.g. via a Dockerfile/entrypoint chown step
  or documented volume-ownership requirement) the non-root shared-root
  volume permission issue from §6. Owner: `docs/shared-hosting.md`,
  `Dockerfile`.
- **P1** — Implement Phase 4A (failure quarantine) from
  `docs/superpowers/specs/2026-09-12-shared-hosting-hardening-design.md`
  before any production rollout. Owner: `crates/valori-node/src/shared.rs`.
- **P1** — Run Phase 1 of that same roadmap (the benchmark/audit harness)
  now that the control-plane gap is confirmed, to inform the integration's
  capacity/placement design. Owner: `benchmarks/`.
- **P2** — Once P0 lands, repeat this verification's Stage 5 (real Free
  signups through the actual control plane) and Stage 6 (Azure), which
  were NOT PERFORMED here for lack of an integration/credentials to test
  against.

## Implemented / tested breakdown

```text
Implemented:            Node-level shared hosting (crates/valori-node/src/shared.rs) — yes.
                         Cloud control-plane integration — no (confirmed absent).
Tested in Rust:          Yes — 9/9 shared_hosting tests, 641/641 full suite, clippy clean.
Tested in local Docker:  Yes — two disposable projects, one container, full isolation
                         matrix, restart recovery, suspend/reactivate — all passed.
Tested through control plane: Not performed — no integration exists to test.
Tested on Azure:         Not performed — no credentials/access available.
Not tested:              Failure-path cases requiring the control-plane integration
                         (worker-unavailable-during-registration, control-plane-crash-
                         mid-provisioning).
Blocked:                 Stage 5 (control-plane E2E) and Stage 6 (Azure) — both blocked
                         on the P0 gap above, not on missing test effort.
```

## Cleanup

Disposable resources created by this verification (left running for
inspection, per instructions — nothing here is production data):

```bash
# Stop and remove the disposable shared-worker container
docker rm -f valori-shvfy-worker-test

# Remove its bind-mounted data directory
rm -rf /tmp/valori-shvfy-shared-data

# Remove the unused named volume created before the permission issue was found
docker volume rm valori-shvfy-shared-root-test-75151

# Remove the locally built test image
docker rmi valori-node:shvfy-test
```

No pre-existing containers, volumes, or data (including the `cloud-*`
e2e stack already running before this verification started) were modified,
restarted, or deleted.
