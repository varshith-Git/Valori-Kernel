# Phase API-3.2 — superseded

This was an earlier, withdrawn write-up of Phase API-3.2. The canonical report is:

**[phase-api-contract-3.2-readiness.md](./phase-api-contract-3.2-readiness.md)**

## Why it was withdrawn

It concluded `sdk_ready: true`, `blocker_count: 0`, and "Zero Contract
Ambiguity". Re-running the phase against the source disproved all three:

- It reported the security contract as sound. The contract declared `401` with
  `body = ApiError` on 70 operations, but `auth_guard_v2` returns a bare
  `StatusCode` — that body has never existed — and `403`, reachable on all 73
  authenticated operations, was documented on none.
- Its non-public route inventory was wrong. It listed `/v1/storage/*`,
  `/v1/snapshot/*`, and `/v1/memory/*` as excluded; the machine-generated
  manifest shows all 13 are `PUBLIC_SDK`.
- It reported public operations as carrying only `read_only`/`read_write`
  scopes. Ten carry `admin`.
- It did not detect that 16 documented responses declare no body while the
  handler returns JSON — the blocker that makes `SDK READY = NO`.

The readiness verdict is computed by `scripts/api-contract-gate.sh` into
`docs/api/sdk-readiness.json`. That file, not a phase document, is the source
of truth.
