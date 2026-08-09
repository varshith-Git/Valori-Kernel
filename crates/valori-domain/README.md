# valori-domain

Cross-boundary domain vocabulary for the Valori platform — the concepts that
the daemon, the node, the model manager, Studio, Cloud, the CLI and the SDKs all
have to agree on.

## What lives here

| Module | Contents |
|---|---|
| `id` | `ProjectId`, `SessionId`, `InstallationId`, `ModelId`, `SnapshotId`; re-exports of `CollectionId`, `NamespaceId`, `ExecutionId` from `valori-core` |
| `error` | `DomainError`, `Result<T>` |

## The admission rule

A type earns a place here only when it is **already represented in two or more
systems, incompatibly**. One consumer means the concept belongs to that
consumer's crate. No consumer means it is not built.

This is not a general-purpose types crate, and it is not a place to park
anything that seems shared. Every type in `id` names its real consumers in its
doc comment.

## Deliberately absent

| Concept | Why not here |
|---|---|
| `RuntimeId` | No runtime identity exists to name. `valori_daemon::Runtime` is keyed by `kind() -> &'static str`, has one implementor, and addresses nodes by project name. Add when a second backend exists *and* runtimes need individual addressing. |
| `PipelineId` | No `Pipeline` platform primitive exists. `valori_ingest::PipelineConfig`/`PipelineResult` are ingest-local and never addressed by id. |
| `OrganizationId`, `UserId`, `BillingAccountId`, `SubscriptionId`, `DeploymentId`, `WorkerId` | Private Cloud control plane. A local Studio project has no organization and no user — Valori runs fully offline. |
| `RecordId`, `NodeId`, `EdgeId`, `ShardId`, `ClusterEpoch` | Kernel identity — they live in `valori-core` and stay there. |

## Position in the dependency graph

`valori-domain` sits **beside** the kernel, not below it:

```
           valori-core   (no_std, zero deps)
            │        │
 valori-kernel      valori-domain   ← std
 (no_std, portable)  │
                     ├── valori-models ── valori-ingest
                     ├── valori-daemon ── desktop (Tauri)
                     └── valori-engine ── valori-node
```

Two rules, both mechanically enforced by
`crates/valori-node/tests/dependency_direction.rs`:

1. **`valori-domain` may depend only on `valori-core`.** If it ever depends on
   `valori-node` or `valori-daemon` it stops being a contract and becomes an
   application.
2. **The determinism-critical crates may not reach it, even transitively** —
   `valori-kernel`, `valori-wire`, `valori-storage`, `valori-state`,
   `valori-index`, `valori-rag`, `valori-verify`. Those crates own the snapshot
   V6, event-log V4 and BLAKE3 audit-chain formats frozen by
   `COMPATIBILITY.md`; product vocabulary must never influence those bytes.

## Wire compatibility

Every ID is `#[serde(transparent)]` — it serializes as the bare primitive it
wraps, never as an object. This is load-bearing: it is what lets these types
replace today's raw `String` fields (such as `ProjectManifest.id`) without
changing a single byte of any existing `project.json`, HTTP response or Cloud
row.

`tests/wire_compat.rs` pins that shape. A failure there is a compatibility
break governed by `COMPATIBILITY.md`, not a test that needs updating.

```bash
cargo test -p valori-domain
```
