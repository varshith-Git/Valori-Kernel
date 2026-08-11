# Phase: Local Cloud E2E Verification

## Goal

Prove the complete Valori Cloud request chain (Python SDK → Cloud API →
project-scoped `vlk_` key → project authorization → worker resolution →
`worker_auth_token` → real `valori-node` → real project data) locally,
against real infrastructure, before any VPS/DNS/production deployment —
and fix whatever real bugs surface along the way.

## Delivered

- `e2e/cloud/` — full Docker Compose environment: real Postgres + real
  migration chains from both repos, real PostgREST, a `/rest/v1` shim,
  two real `valori-node` workers (512MB/0.5CPU each, isolated volumes),
  the real Next.js Cloud API (new `Dockerfile.e2e`), and a Python test
  runner. `docker compose down -v && build --no-cache && up -d` proven
  reproducible from a clean state.
- 48 pytest tests across 9 files (`test_auth`, `test_collections`,
  `test_limits`, `test_persistence`, `test_projects`, `test_regressions`,
  `test_security`, `test_vectors`, `test_worker_auth`) — 46 pass, 2
  skipped (persistence tests need Docker access the `python` service
  deliberately doesn't have; verified manually from the host instead,
  see Validation).
- 6 real production bugs found and fixed (2 in `Valori-Kernel`, 4 in the
  private `valori-ui` repo) — see CHANGELOG's "Fixed (Local Cloud E2E
  verification)" entry for the full list.
- `docs/architecture/project-api-v1.md` — frozen v1 API contract,
  documenting only routes actually exercised.
- `e2e/cloud/scripts/scan_logs_for_secrets.sh` — real service log sweep
  for raw `vlk_` keys / worker tokens; found and fixed the `NodeConfig`
  leak (see below).
- One real resource-benchmark data point (dim=384, 10K vectors,
  BruteForce, real 512MB/0.5CPU container) — see
  `e2e/cloud/results/BENCHMARK_SCOPE.md` for what was and wasn't
  measured, and why.
- Worker routing isolation and restart persistence verified against the
  real containers (see Validation).

## Findings

1. **`create_api_key()` was completely broken** (ambiguous-column SQL,
   400 on every call) — never caught because nothing had exercised it
   against real PostgREST before this phase.
2. **`import valoricore` required a compiled Rust extension** even for
   the documented pure-HTTP usage — a packaging bug, not by design.
3. **`proxyToNode()` silently converted DELETE to POST** and crashed on
   any real 204 response — both latent until a real DELETE-returning-204
   route was exercised.
4. **`valori-node` logged its own worker auth token in plaintext** on
   every startup — found by the log-sweep step, not by inspection.
5. **State hash did not match before/after a worker restart**, even
   though the underlying vector data and search results were
   byte-identical. Root cause not isolated this session — see
   Validation and Follow-ups.
6. **Rate limiting and the per-project API-key cap are real** (60/min
   per key; 3 active keys per project) — not gaps, but real limits this
   phase's own test design initially fought by sharing state across
   unrelated tests.

## Validation

- `docker compose run --rm python pytest -v` → **46 passed, 2 skipped**
  (from a `docker compose down -v && build --no-cache && up -d` clean
  state — reproducibility confirmed, not just a warm-cache run).
- `cargo test --workspace` → **1186 passed, 0 failed**.
- `cargo check --workspace` → clean.
- `cargo clippy --workspace` → 0 warnings, 0 errors.
- `cargo build -p valori-kernel --target wasm32-unknown-unknown` → clean
  (no_std invariant intact).
- `python/tests` (SDK unit tests) → 74 passed, 8 skipped, 1 failed
  (`test_concurrency_stress_local`) — confirmed via `git stash` that this
  same test fails identically on the pre-phase code; not a regression
  from this phase's changes.
- `npx tsc --noEmit` (valori-ui/ui) → clean.
- `npm run build` (valori-ui/ui) → succeeds.
- Worker-direct auth: 11/11 tests pass against the real Worker A
  container (no token → 401, wrong token → 401, customer `vlk_` key →
  401, correct token → success; `/health` confirmed intentionally
  unauthenticated by design, not changed).
- Worker routing: real `docker compose stop worker-a` → Project A search
  returns 503, Project B (worker-b) unaffected, returns 200. No
  auto-reroute exists or was added.
- Restart persistence: real `docker compose stop/start worker-a` →
  vector IDs and search results identical before/after. State hash
  (`/v1/proof/state`) did **not** match before/after — reported as a
  genuine, unresolved finding (see Follow-ups), not glossed over.
- Secret log sweep: found a real leak (`NodeConfig`'s derived `Debug`),
  fixed it, re-ran after rebuild — clean across all 6 real services.
- Resource benchmark: one real data point only (dim=384, 10K vectors,
  BruteForce) — the full 36-combination matrix was not run; see
  `e2e/cloud/results/BENCHMARK_SCOPE.md` for the honest scope
  explanation (multi-hour real compute, not fabricated).

## Follow-ups

- **State-hash mismatch across worker restart** (Finding 5) needs its
  own root-cause session: isolate whether it's an artifact of
  worker-a's volume being shared/cumulative across this whole test
  session (unrelated events landing between the two snapshots) or a
  genuine snapshot-restore determinism bug in `valori-kernel`. Given
  CLAUDE.md's own stated invariant ("state hash is reproduced from
  scratch after snapshot restore"), this should be treated as
  potentially serious until explained.
- **Full resource-benchmark matrix** (3 dims × 3 vector counts × 4 index
  types) — not run this phase; needs a dedicated multi-hour session or
  CI job using the same real-container pattern established here.
- **5GB storage quota testing** — not implemented; Docker Desktop on
  macOS doesn't expose a hard per-volume byte quota to `docker compose`
  without extra host-level setup this phase didn't build.
- API-key management (create/revoke/expiry) has no project-isolation
  check to test, by design — it's an org-role check via PostgREST RPCs
  directly, not a `/api/projects/[id]/*` route. Documented in
  `project-api-v1.md`, not force-fit into the isolation matrix.
