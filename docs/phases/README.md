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
| 1.5 — Crypto-shredding design (GDPR) | — | — | ⬜ next |
| 1.6 — Security design doc | — | — | ⬜ planned |
| 1.7 — Verifier hardening (limits + fuzzing) | — | — | ⬜ planned |
| 1.8 — Storage policy (snapshot cadence, zstd, disk-full) | — | — | ⬜ planned |
| 1.9 — Committer trait seam | — | — | ⬜ planned |
| 1.10 — CI upgrades (multi-arch hash equality, cargo-deny) | — | — | ⬜ planned |
| 1.11 — Docker + compose | — | — | ⬜ planned |
| 2.x — Cluster mode (openraft) | — | — | ⬜ planned |

## Report template

Every report answers five questions:

1. **Goal** — what this phase was supposed to achieve (1–2 sentences)
2. **Delivered** — what actually landed, file by file where it matters
3. **Findings** — bugs and design gaps discovered during the work
   (often the most valuable section)
4. **Validation** — the evidence: test counts, demos, end-to-end runs
5. **Follow-ups** — anything consciously deferred, and to which phase
