# Security rules

**Source of truth:** [`docs/THREAT_MODEL.md`](../../docs/THREAT_MODEL.md) — explicit in-scope/out-of-scope
lists, verified current. Read it before touching auth, the cluster write path, or anything
namespace/tenant-boundary-adjacent. This file is the operational checklist on top of it.

## What's actually in scope here (verified — don't assume more or less)

From `THREAT_MODEL.md`'s "In scope" list: silent state drift across replicas, tampered audit log,
snapshot corruption, replay of duplicate commands, unauthorized writes in cluster mode, namespace
cross-contamination (multi-tenancy within one node/cluster via `NamespaceId`).

**Explicitly out of scope today** (per the same doc — do not silently assume these are handled):
a privileged OS-level attacker, a compromised leader node, transport confidentiality (HTTP is plaintext
by default), authentication/authorization strength, embedding-model confidentiality, write-volume DoS.
If a task implies one of these is already mitigated, it isn't — say so rather than building on that
assumption.

## Never do this

- **Never construct a `KernelEvent` outside `KernelState::apply_event_ns()`** (`INVARIANTS.md` I-04). Any
  code path that writes to the audit log without going through the kernel apply path is both a
  determinism bug and a security bug — it bypasses whatever integrity guarantee the BLAKE3 chain provides.
- **Never write an audit entry for a rejected or duplicate event** (`AGENTS.md` invariant 1: dedup check →
  kernel apply → audit write, in that order, always).
- **Never skip `request_id` dedup on a new cluster command** (`AGENTS.md` invariant 6) — this is what makes
  retried writes safe; skipping it reopens the duplicate-replay threat `THREAT_MODEL.md` names explicitly.
- **Never add a fourth namespace-isolation checkpoint casually.** Namespace isolation is enforced at
  exactly three points today (`apply_committed_event_ns()`, WAL replay, `build_index()` post-restore —
  `AGENTS.md` invariant 2). If you add a new state-reconstruction path, it needs its own guard, not an
  assumption that one of the existing three covers it.
- **Never trust a client-supplied namespace/collection identity without going through the same resolution
  path every other handler uses** — cross-namespace leakage is explicitly in scope as a threat, not a
  hypothetical.

## Object storage & credentials

`VALORI_OBJECT_STORE_URL` carries S3/MinIO/R2/B2 credentials via env vars (`VALORI_OBJECT_STORE_ENDPOINT`,
region, etc. — see `AGENTS.md`'s environment variable table). Never put these values in committed config,
example files with real values, or log output. `docs/DR.md` documents the disaster-recovery contract these
credentials protect — an object-store credential leak is a durability *and* confidentiality problem for
every project using shared object storage.

## Auth

`VALORI_AUTH_TOKEN` is the node's own bearer-token auth (optional — absence means no auth, which is a
valid and common local/dev configuration, not a bug to "fix" uninvited). Cluster peer traffic can be
secured with mTLS (`VALORI_TLS_CA`/`VALORI_TLS_CERT`/`VALORI_TLS_KEY`). Neither is a substitute for the
other — bearer token authenticates HTTP clients, mTLS authenticates Raft peers.

## Cargo-level supply-chain gate

`cargo deny check` (config: `deny.toml`, enforced on every PR via `.github/workflows/cargo-deny.yml`)
checks licenses and security advisories for every dependency, including optional ones (the `[graph]`
section builds with all features enabled specifically so feature-gated deps aren't skipped). A new
dependency failing this gate is a real finding, not a false positive to silence without reading why.

## Reviewing a change for security impact

Use `.claude/agents/security-reviewer.md` for anything touching: the cluster write path
(`ValoriStateMachine::apply`, `raft.client_write`), namespace resolution, object-store credential
handling, or `VALORI_AUTH_TOKEN` verification. Don't invent a vulnerability without a concrete code path —
`THREAT_MODEL.md`'s in-scope list is the frame for what's a real finding here vs. speculative.
