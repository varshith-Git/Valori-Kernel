@AGENTS.md

---

The import above is the substantive, tool-neutral repository map, invariants, commands, and SDK
reference — read it, it's not optional background. Everything below is Claude-Code-specific: where the
deeper policy lives, when to invoke a skill or a specialist reviewer, and what's automatically enforced.

## What this repository is

Valori: a deterministic, `no_std` Rust kernel (fixed-point vector store + knowledge graph + BLAKE3 audit
chain) wrapped in an HTTP server (`valori-node`) that runs standalone or as a Raft cluster. Also ships a
desktop app (Tauri + Next.js UI), Python/TypeScript SDKs, and a CLI. **This repo has no SaaS/billing
control plane** — no Postgres, no Supabase, no tenant provisioning. That's a separate repo (`valori-ui`).
If a task's premise assumes cloud provisioning/billing infrastructure lives here, it doesn't — say so.

## Repository map

| Area | Path |
|---|---|
| Deterministic kernel | `crates/valori-kernel` (`no_std` — see invariants) |
| HTTP server + cluster orchestration | `crates/valori-node` |
| Raft consensus | `crates/valori-consensus` |
| Durable storage (WAL, event log, object store) | `crates/valori-storage` |
| Control-plane persistence for this node (redb) | `crates/valori-metadata` |
| Desktop-product UI | `ui/` (Next.js) |
| Desktop app wrapper | `desktop/` (Tauri) — `valori-daemon` is its local lifecycle manager, not a cloud service |
| Python SDK | `python/valoricore` |
| API contract | `api/openapi/valori-v1.yaml` |
| Docs | `docs/` — `docs/architecture/layers.md` is the normative architecture doc; `docs/README.md` maps the rest |
| K8s / Terraform | `deploy/helm/valori`, `terraform/{aws,azure}` |

Full crate table with one-liners and key files: `AGENTS.md`'s "Crate layout" / "Key files" sections.

## Architectural boundaries

`docs/architecture/layers.md` is the normative architecture document (dependency graph, per-crate
ownership, a "Never do this" section) — read it before any structural change, don't re-derive it from
memory. `.claude/rules/architecture.md` is a thin pointer to it plus the operational checklist. The two
execution paths — standalone (`server.rs`, direct engine access) vs. cluster (`cluster_server.rs`, Raft) —
are architecturally real, not a detail; see `AGENTS.md`'s "MANDATORY: single-node AND multi-node" section
before adding any endpoint.

## Critical invariants

The authoritative list is [`INVARIANTS.md`](INVARIANTS.md) — 15 numbered, code-referenced invariants.
The single highest-impact one for day-to-day work: **only `KernelState::apply_event_ns()` may construct a
`KernelEvent`** (I-04) — everything else (dedup, audit ordering, replay-is-the-only-reconstruction-path,
`no_std`) follows from `.claude/rules/determinism.md`'s checklist. Read that file — not this paragraph —
before touching `valori-kernel`, WAL, snapshots, or hashing.

## How to work in this repository

1. Inspect the actual implementation first — this repo has extensive, current docs (`docs/`,
   `INVARIANTS.md`, `docs/architecture/layers.md`); check them before assuming.
2. Identify which invariants govern the code you're touching (`.claude/rules/determinism.md`'s table maps
   "what you're touching" → "what to re-read" → "what to run").
3. Make the smallest coherent change — see the Behavioral Guidelines imported above (Simplicity First,
   Surgical Changes).
4. Update tests per `.claude/rules/testing.md`'s change-to-test matrix.
5. Run the verification that matrix actually requires — name what you ran, not just "tests pass."
6. Summarize exactly what changed.

## Commands

Verified, not invented — see `AGENTS.md`'s "Commands" section and `.claude/rules/testing.md`'s full list.
The two you'll use most: `cargo test -p valori-kernel -p valori-node`, `cd ui && npx tsc --noEmit -p .`.

## Rule references

| File | Covers |
|---|---|
| [`.claude/rules/architecture.md`](.claude/rules/architecture.md) | System boundaries, durable vs. ephemeral state, trust boundaries |
| [`.claude/rules/rust.md`](.claude/rules/rust.md) | Panic policy, `unsafe`, `no_std` boundary, dependency policy |
| [`.claude/rules/determinism.md`](.claude/rules/determinism.md) | The mandatory review checklist — what to re-read/run per change type |
| [`.claude/rules/api.md`](.claude/rules/api.md) | The three-way OpenAPI contract gate, error envelope, SDK generation |
| [`.claude/rules/frontend.md`](.claude/rules/frontend.md) | `ui/` theming, surface ladder, elevation — points to `ui/CLAUDE.md` |
| [`.claude/rules/security.md`](.claude/rules/security.md) | What's actually in/out of scope per `docs/THREAT_MODEL.md`, never-do list |
| [`.claude/rules/testing.md`](.claude/rules/testing.md) | Change → required verification matrix |

## Skills

Invoke when the task matches, not preemptively:

| Skill | When |
|---|---|
| `.claude/skills/release/` | Cutting a `valori-node` or Python SDK release — explicitly requested only |
| `.claude/skills/deploy-control-plane/` | Raft cluster membership changes (add/remove node) |
| `.claude/skills/deploy-worker/` | Rolling out a `valori-node` version to a host/cluster/K8s deployment |
| `.claude/skills/migration/` | A snapshot/wire-format version change (not SQL — this repo has none) |
| `.claude/skills/benchmark/` | Measuring throughput/latency/recall/determinism — see the real script table before writing a new one |

## Specialist reviewers

Dispatch proactively for the matching diff, not only when asked: `.claude/agents/kernel-reviewer.md`
(anything on the determinism/snapshot/WAL/hashing path), `.claude/agents/security-reviewer.md` (auth,
cluster write path, namespace isolation, object-store credentials), `.claude/agents/cloud-reviewer.md`
(Helm/Terraform/Compose, Raft membership, durability of a deployment change),
`.claude/agents/ui-reviewer.md` (anything in `ui/**`). All four review only — they don't implement fixes.

## Safety

`.claude/hooks/dangerous-command-check.sh` blocks (`PreToolUse`, exit 2) a specific, documented set of
destructive command patterns — `rm -rf`, `git reset --hard`/`clean -fd`/`push --force`, `DROP`/`TRUNCATE`,
`docker compose down -v`/`volume rm`/`system prune`, `terraform destroy`, `kubectl delete namespace`,
`helm uninstall`. It classifies rather than pattern-matching naively (see the script's own comments for
why each class is there). `.claude/hooks/rust-check.sh` runs `cargo fmt --check` + a scoped `cargo check`
after editing a `.rs` file (`PostToolUse`) — fast, not a full test-suite run per edit. Neither hook is a
substitute for the verification in `.claude/rules/testing.md`; they catch a narrower, faster class of
problem. A hook block is not final — if the destructive action is genuinely intended, get the user's
explicit approval and proceed some other way; don't route around the hook silently.
