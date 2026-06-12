# Phase Reports

One report per delivered phase of the multi-node roadmap
([docs/MULTINODE_ROADMAP.md](../MULTINODE_ROADMAP.md)). Each report records
what shipped, what was found along the way, and the validation evidence —
so the history of *why* the codebase looks the way it does survives the
people and sessions that built it.

## Status

| Phase | Report | Commit | Status |
|---|---|---|---|
| 0 — Baseline durability & verifier | [phase-0-baseline.md](phase-0-baseline.md) | merged via PR #3 (`57da43e`) | ✅ done |
| 1.1 — Workspace restructure | [phase-1.1-workspace-restructure.md](phase-1.1-workspace-restructure.md) | `2bd793d` | ✅ done |
| 1.1b — Per-crate test layout + kernel fixes | [phase-1.1b-per-crate-tests.md](phase-1.1b-per-crate-tests.md) | `1db62c9` | ✅ done |
| 1.2 — valori-wire + segment format v3 | [phase-1.2-valori-wire-v3.md](phase-1.2-valori-wire-v3.md) | `b4ac53b` | ✅ done |
| 1.3 — FxpFormat seam (configurable precision) | [phase-1.3-fxpformat-seam.md](phase-1.3-fxpformat-seam.md) | `22f600b` | ✅ done |
| 1.4 — Collections seam | [phase-1.4-collections-seam.md](phase-1.4-collections-seam.md) | `41fe5b6` | ✅ done |
| 1.5 — Crypto-shredding design (GDPR) | [phase-1.5-crypto-shredding.md](phase-1.5-crypto-shredding.md) | `003ce7e` | ✅ done |
| 1.6 — Security design doc | [phase-1.6-security-model.md](phase-1.6-security-model.md) | see git log | ✅ done |
| 1.7 — Verifier hardening (limits + fuzzing) | [phase-1.7-verifier-hardening.md](phase-1.7-verifier-hardening.md) | see git log | ✅ done |
| 1.8 — Storage policy (snapshot cadence, zstd, disk-full) | [phase-1.8-storage-policy.md](phase-1.8-storage-policy.md) | see git log | ✅ done |
| 1.9 — Committer trait seam | [phase-1.9-committer-trait.md](phase-1.9-committer-trait.md) | see git log | ✅ done |
| 1.10 — CI upgrades (multi-arch hash equality, cargo-deny) | [phase-1.10-ci-upgrades.md](phase-1.10-ci-upgrades.md) | see git log | ✅ done |
| 1.11 — Docker + compose | [phase-1.11-docker-compose.md](phase-1.11-docker-compose.md) | see git log | ✅ done |
| 2.1 — openraft type config | [phase-2.1-openraft-types.md](phase-2.1-openraft-types.md) | see git log | ✅ done |
| 2.2 — Raft log store | [phase-2.2-raft-log-store.md](phase-2.2-raft-log-store.md) | see git log | ✅ done |
| 2.3 — Raft state machine (kernel + audit) | [phase-2.3-raft-state-machine.md](phase-2.3-raft-state-machine.md) | see git log | ✅ done |
| 2.4 — gRPC transport (tonic) | [phase-2.4-grpc-transport.md](phase-2.4-grpc-transport.md) | see git log | ✅ done |
| 2.5 — RaftCommitter + cluster bootstrap | [phase-2.5-raft-committer.md](phase-2.5-raft-committer.md) | see git log | ✅ done |
| 2.6 — Cluster management API + engine wiring | — | — | ⬜ next |
| 2.7 — Snapshot transfer | — | — | ⬜ planned |
| 2.8 — Turmoil fault-tolerance tests | — | — | ⬜ planned |
| 2.9 — Admin audit events in chain | — | — | ⬜ planned |
| 2.10 — Production hardening (mTLS, persistent log, metrics) | — | — | ⬜ planned |

## Report template

Every report answers five questions:

1. **Goal** — what this phase was supposed to achieve (1–2 sentences)
2. **Delivered** — what actually landed, file by file where it matters
3. **Findings** — bugs and design gaps discovered during the work
   (often the most valuable section)
4. **Validation** — the evidence: test counts, demos, end-to-end runs
5. **Follow-ups** — anything consciously deferred, and to which phase
