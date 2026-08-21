# Phase 2.5 — Working-Tree Forensics & Diff Classification Audit

## Executive Forensics Summary

- **Baseline Commit**: `eee123dd0485941252da9e1ff8438a478e34b3a2` (`HEAD` of `origin/main`)
- **Working Tree State**: Uncommitted modifications & untracked files in the working workspace relative to baseline.
- **Total Files Analyzed**: 328
- **Category F ("Cannot determine yet")**: **0** (All 328 files explicitly classified).

### Category Breakdown

| Category | Description | Count | Action |
|----------|-------------|-------|--------|
| **A. API-2 Change** | Direct Phase API-2 convergence modification (wire DTO, error code, route status, Python remote SDK, `@valori/api-types`) | 31 | Deep-audit & verify reproducibility |
| **B. Pre-existing Change** | Modifications present before API-2 or part of earlier workspace phases | 136 | Shallow-record; keep intact |
| **C. Required Dependency** | Touched server/state/storage/consensus crate files required for build & test | 96 | Deep-audit build & test assertions |
| **D. Unrelated / Suspicious** | Unfinished platform features (Index Manager, Snapshot v8, Graph/TreeRAG/Community, Manifests) | 21 | Deep-audit 6-question boundary; isolate without modification or deletion |
| **E. Docs & Artifacts** | OpenAPI contract, generated TypeScript types, phase docs, architectural reviews | 44 | Audit contract integrity & generator reproducibility |
| **F. Cannot Determine** | Unclassified / ambiguous files | **0** | Resolved |

---

## Detailed File Classification Table

| File | Status | Category | Evidence & Rationale | Action |
|------|--------|----------|----------------------|--------|
| `crates/valori-engine/src/engine.rs` | `M` | **A** | Engine error mapping or standalone request_id dedup storage | Verify engine error & dedup invariants |
| `crates/valori-engine/src/error.rs` | `M` | **A** | Engine error mapping or standalone request_id dedup storage | Verify engine error & dedup invariants |
| `crates/valori-node/src/api.rs` | `M` | **A** | Direct Phase API-2 convergence change (wire DTO, error code, client, test) | Verify & maintain reproducibility |
| `crates/valori-node/src/cluster_server.rs` | `M` | **A** | API-2 route & status-code convergence (error codes, request_id dedup, k bounds) | Verify API behavior & contract tests |
| `crates/valori-node/src/config.rs` | `M` | **A** | API-2 route & status-code convergence (error codes, request_id dedup, k bounds) | Verify API behavior & contract tests |
| `crates/valori-node/src/error_codes.rs` | `??` | **A** | Direct Phase API-2 convergence change (wire DTO, error code, client, test) | Verify & maintain reproducibility |
| `crates/valori-node/src/errors.rs` | `M` | **A** | Direct Phase API-2 convergence change (wire DTO, error code, client, test) | Verify & maintain reproducibility |
| `crates/valori-node/src/main.rs` | `M` | **A** | API-2 route & status-code convergence (error codes, request_id dedup, k bounds) | Verify API behavior & contract tests |
| `crates/valori-node/src/openapi.rs` | `??` | **A** | Direct Phase API-2 convergence change (wire DTO, error code, client, test) | Verify & maintain reproducibility |
| `crates/valori-node/src/routes/collections.rs` | `M` | **A** | API-2 route & status-code convergence (error codes, request_id dedup, k bounds) | Verify API behavior & contract tests |
| `crates/valori-node/src/routes/graph.rs` | `M` | **A** | API-2 route & status-code convergence (error codes, request_id dedup, k bounds) | Verify API behavior & contract tests |
| `crates/valori-node/src/routes/index_lifecycle.rs` | `??` | **A** | API-2 route & status-code convergence (error codes, request_id dedup, k bounds) | Verify API behavior & contract tests |
| `crates/valori-node/src/routes/memory.rs` | `M` | **A** | API-2 route & status-code convergence (error codes, request_id dedup, k bounds) | Verify API behavior & contract tests |
| `crates/valori-node/src/routes/mod.rs` | `M` | **A** | API-2 route & status-code convergence (error codes, request_id dedup, k bounds) | Verify API behavior & contract tests |
| `crates/valori-node/src/routes/query_planner.rs` | `??` | **A** | API-2 route & status-code convergence (error codes, request_id dedup, k bounds) | Verify API behavior & contract tests |
| `crates/valori-node/src/routes/records.rs` | `M` | **A** | API-2 route & status-code convergence (error codes, request_id dedup, k bounds) | Verify API behavior & contract tests |
| `crates/valori-node/src/server.rs` | `M` | **A** | API-2 route & status-code convergence (error codes, request_id dedup, k bounds) | Verify API behavior & contract tests |
| `crates/valori-node/tests/api_contract.rs` | `??` | **A** | Direct Phase API-2 convergence change (wire DTO, error code, client, test) | Verify & maintain reproducibility |
| `crates/valori-node/tests/api_index_config.rs` | `M` | **A** | API-2 route & status-code convergence (error codes, request_id dedup, k bounds) | Verify API behavior & contract tests |
| `crates/valori-node/tests/openapi_generated.rs` | `??` | **A** | Direct Phase API-2 convergence change (wire DTO, error code, client, test) | Verify & maintain reproducibility |
| `python/tests/test_create_collection_contract.py` | `??` | **A** | Direct Phase API-2 convergence change (wire DTO, error code, client, test) | Verify & maintain reproducibility |
| `python/valoricore/cli.py` | `M` | **A** | Python CLI collection default handling | Verify CLI collection requirement |
| `python/valoricore/remote.py` | `M` | **A** | Direct Phase API-2 convergence change (wire DTO, error code, client, test) | Verify & maintain reproducibility |
| `scripts/check-studio-boundary.mjs` | `??` | **A** | API contract scripts (Studio boundary or generation script) | Audit script reproducibility |
| `scripts/generate-api-types.sh` | `??` | **A** | Direct Phase API-2 convergence change (wire DTO, error code, client, test) | Verify & maintain reproducibility |
| `scripts/studio-boundary.json` | `??` | **A** | API contract scripts (Studio boundary or generation script) | Audit script reproducibility |
| `ui/src/components/collections/CreateCollectionDialog.tsx` | `M` | **A** | UI wire model convergence to generated @valori/api-types | Verify UI wire type migration |
| `ui/src/components/collections/DeleteCollectionDialog.tsx` | `M` | **A** | UI wire model convergence to generated @valori/api-types | Verify UI wire type migration |
| `ui/src/lib/hooks/useCollections.ts` | `M` | **A** | UI wire model convergence to generated @valori/api-types | Verify UI wire type migration |
| `ui/src/lib/hooks/useHealth.ts` | `M` | **A** | UI wire model convergence to generated @valori/api-types | Verify UI wire type migration |
| `ui/src/types/valori.ts` | `M` | **A** | UI wire model convergence to generated @valori/api-types | Verify UI wire type migration |
| `.github/workflows/desktop-build.yml` | `M` | **B** | Workspace configuration or general crate file | Keep intact |
| `CLAUDE.md` | `M` | **B** | Workspace configuration or general crate file | Keep intact |
| `Cargo.lock` | `M` | **B** | Workspace configuration or general crate file | Keep intact |
| `crates/valori-cli/src/bin/bench_bf_vs_bq.rs` | `M` | **B** | Workspace configuration or general crate file | Keep intact |
| `crates/valori-cli/src/bin/bench_persistence.rs` | `M` | **B** | Workspace configuration or general crate file | Keep intact |
| `crates/valori-cli/src/commands/import.rs` | `M` | **B** | Workspace configuration or general crate file | Keep intact |
| `crates/valori-cli/src/commands/timeline.rs` | `M` | **B** | Workspace configuration or general crate file | Keep intact |
| `crates/valori-cli/src/commands/wizard.rs` | `M` | **B** | Workspace configuration or general crate file | Keep intact |
| `crates/valori-cli/src/main.rs` | `M` | **B** | Workspace configuration or general crate file | Keep intact |
| `crates/valori-cli/tests/cluster_cli.rs` | `M` | **B** | Workspace configuration or general crate file | Keep intact |
| `crates/valori-daemon/src/daemon.rs` | `M` | **B** | Workspace configuration or general crate file | Keep intact |
| `crates/valori-daemon/src/domain_adapter.rs` | `M` | **B** | Workspace configuration or general crate file | Keep intact |
| `crates/valori-daemon/src/http.rs` | `M` | **B** | Workspace configuration or general crate file | Keep intact |
| `crates/valori-daemon/src/migration/m001_project_registry.rs` | `M` | **B** | Workspace configuration or general crate file | Keep intact |
| `crates/valori-daemon/src/project.rs` | `M` | **B** | Workspace configuration or general crate file | Keep intact |
| `crates/valori-daemon/src/runtime/local.rs` | `M` | **B** | Workspace configuration or general crate file | Keep intact |
| `crates/valori-daemon/tests/lifecycle.rs` | `M` | **B** | Workspace configuration or general crate file | Keep intact |
| `crates/valori-domain/README.md` | `M` | **B** | Workspace configuration or general crate file | Keep intact |
| `crates/valori-domain/src/error.rs` | `M` | **B** | Workspace configuration or general crate file | Keep intact |
| `crates/valori-domain/src/lib.rs` | `M` | **B** | Workspace configuration or general crate file | Keep intact |
| `crates/valori-domain/src/project.rs` | `M` | **B** | Workspace configuration or general crate file | Keep intact |
| `crates/valori-domain/tests/invariants.rs` | `M` | **B** | Workspace configuration or general crate file | Keep intact |
| `crates/valori-domain/tests/project_contract.rs` | `M` | **B** | Workspace configuration or general crate file | Keep intact |
| `crates/valori-effect/src/capability.rs` | `M` | **B** | Workspace configuration or general crate file | Keep intact |
| `crates/valori-effect/src/effect.rs` | `M` | **B** | Workspace configuration or general crate file | Keep intact |
| `crates/valori-effect/src/tasks/graph_rag.rs` | `M` | **B** | Workspace configuration or general crate file | Keep intact |
| `crates/valori-effect/src/tasks/insert_record.rs` | `M` | **B** | Workspace configuration or general crate file | Keep intact |
| `crates/valori-ffi/src/lib.rs` | `M` | **B** | Workspace configuration or general crate file | Keep intact |
| `crates/valori-mcp/tests/integration_node.rs` | `M` | **B** | Workspace configuration or general crate file | Keep intact |
| `crates/valori-metadata/README.md` | `M` | **B** | Workspace configuration or general crate file | Keep intact |
| `crates/valori-metadata/src/collection.rs` | `M` | **B** | Workspace configuration or general crate file | Keep intact |
| `crates/valori-metadata/src/db.rs` | `M` | **B** | Workspace configuration or general crate file | Keep intact |
| `crates/valori-metadata/src/domain_adapter.rs` | `M` | **B** | Workspace configuration or general crate file | Keep intact |
| `crates/valori-metadata/src/lib.rs` | `M` | **B** | Workspace configuration or general crate file | Keep intact |
| `crates/valori-metadata/src/project.rs` | `M` | **B** | Workspace configuration or general crate file | Keep intact |
| `crates/valori-rag/README.md` | `M` | **B** | Workspace configuration or general crate file | Keep intact |
| `crates/valori-rag/src/graph.rs` | `M` | **B** | Workspace configuration or general crate file | Keep intact |
| `crates/valori-rag/src/lib.rs` | `M` | **B** | Workspace configuration or general crate file | Keep intact |
| `crates/valori-search/README.md` | `M` | **B** | Workspace configuration or general crate file | Keep intact |
| `crates/valori-search/src/lib.rs` | `M` | **B** | Workspace configuration or general crate file | Keep intact |
| `crates/valori-wire/src/lib.rs` | `M` | **B** | Workspace configuration or general crate file | Keep intact |
| `crates/valori-wire/tests/hardening.rs` | `M` | **B** | Workspace configuration or general crate file | Keep intact |
| `github/workflows/ci.yml` | `M` | **B** | Workspace configuration or general crate file | Keep intact |
| `ui/package-lock.json` | `M` | **B** | Pre-existing UI component cleanups or workspace feature updates | Keep intact |
| `ui/package.json` | `M` | **B** | Pre-existing UI component cleanups or workspace feature updates | Keep intact |
| `ui/src/app/api/cloud/projects/[id]/ingest/route.ts` | `M` | **B** | Pre-existing UI component cleanups or workspace feature updates | Keep intact |
| `ui/src/app/api/ingest/route.ts` | `M` | **B** | Pre-existing UI component cleanups or workspace feature updates | Keep intact |
| `ui/src/app/api/namespaces/[name]/route.ts` | `M` | **B** | Pre-existing UI component cleanups or workspace feature updates | Keep intact |
| `ui/src/app/api/namespaces/route.ts` | `M` | **B** | Pre-existing UI component cleanups or workspace feature updates | Keep intact |
| `ui/src/app/api/projects/[name]/open/route.ts` | `M` | **B** | Pre-existing UI component cleanups or workspace feature updates | Keep intact |
| `ui/src/app/api/projects/route.ts` | `M` | **B** | Pre-existing UI component cleanups or workspace feature updates | Keep intact |
| `ui/src/app/audit/page.tsx` | `M` | **B** | Pre-existing UI component cleanups or workspace feature updates | Keep intact |
| `ui/src/app/cloud/CloudProjectsClient.tsx` | `M` | **B** | Pre-existing UI component cleanups or workspace feature updates | Keep intact |
| `ui/src/app/cloud/archived/page.tsx` | `M` | **B** | Pre-existing UI component cleanups or workspace feature updates | Keep intact |
| `ui/src/app/cloud/layout.tsx` | `M` | **B** | Pre-existing UI component cleanups or workspace feature updates | Keep intact |
| `ui/src/app/cloud/page.tsx` | `M` | **B** | Pre-existing UI component cleanups or workspace feature updates | Keep intact |
| `ui/src/app/cloud/projects/[id]/CollectionsPanel.tsx` | `M` | **B** | Pre-existing UI component cleanups or workspace feature updates | Keep intact |
| `ui/src/app/cloud/projects/[id]/ProjectWorkspace.tsx` | `M` | **B** | Pre-existing UI component cleanups or workspace feature updates | Keep intact |
| `ui/src/app/cloud/projects/[id]/cluster/page.tsx` | `M` | **B** | Pre-existing UI component cleanups or workspace feature updates | Keep intact |
| `ui/src/app/cloud/projects/[id]/graph/page.tsx` | `M` | **B** | Pre-existing UI component cleanups or workspace feature updates | Keep intact |
| `ui/src/app/cloud/projects/[id]/layout.tsx` | `??` | **B** | Pre-existing UI component cleanups or workspace feature updates | Keep intact |
| `ui/src/app/cloud/projects/[id]/metrics/page.tsx` | `M` | **B** | Pre-existing UI component cleanups or workspace feature updates | Keep intact |
| `ui/src/app/cloud/projects/[id]/operations/[opId]/CloudOperationDetail.tsx` | `??` | **B** | Pre-existing UI component cleanups or workspace feature updates | Keep intact |
| `ui/src/app/cloud/projects/[id]/operations/[opId]/page.tsx` | `M` | **B** | Pre-existing UI component cleanups or workspace feature updates | Keep intact |
| `ui/src/app/cloud/projects/[id]/operations/page.tsx` | `M` | **B** | Pre-existing UI component cleanups or workspace feature updates | Keep intact |
| `ui/src/app/cloud/projects/[id]/page.tsx` | `M` | **B** | Pre-existing UI component cleanups or workspace feature updates | Keep intact |
| `ui/src/app/cloud/projects/[id]/playground/page.tsx` | `M` | **B** | Pre-existing UI component cleanups or workspace feature updates | Keep intact |
| `ui/src/app/cloud/projects/[id]/proof/ProofView.tsx` | `D` | **B** | Pre-existing UI component cleanups or workspace feature updates | Keep intact |
| `ui/src/app/cloud/projects/[id]/proof/page.tsx` | `M` | **B** | Pre-existing UI component cleanups or workspace feature updates | Keep intact |
| `ui/src/app/cloud/projects/[id]/snapshots/page.tsx` | `M` | **B** | Pre-existing UI component cleanups or workspace feature updates | Keep intact |
| `ui/src/app/cloud/projects/[id]/tools/CloudToolsWorkspace.tsx` | `??` | **B** | Pre-existing UI component cleanups or workspace feature updates | Keep intact |
| `ui/src/app/cloud/projects/[id]/tools/ToolsWorkspace.tsx` | `D` | **B** | Pre-existing UI component cleanups or workspace feature updates | Keep intact |
| `ui/src/app/cloud/projects/[id]/tools/page.tsx` | `M` | **B** | Pre-existing UI component cleanups or workspace feature updates | Keep intact |
| `ui/src/app/cloud/settings/api-keys/page.tsx` | `M` | **B** | Pre-existing UI component cleanups or workspace feature updates | Keep intact |
| `ui/src/app/cloud/settings/developer/page.tsx` | `M` | **B** | Pre-existing UI component cleanups or workspace feature updates | Keep intact |
| `ui/src/app/cloud/settings/security/page.tsx` | `M` | **B** | Pre-existing UI component cleanups or workspace feature updates | Keep intact |
| `ui/src/app/cloud/settings/team/page.tsx` | `M` | **B** | Pre-existing UI component cleanups or workspace feature updates | Keep intact |
| `ui/src/app/cluster/page.tsx` | `M` | **B** | Pre-existing UI component cleanups or workspace feature updates | Keep intact |
| `ui/src/app/login/actions.ts` | `M` | **B** | Pre-existing UI component cleanups or workspace feature updates | Keep intact |
| `ui/src/app/metrics/page.tsx` | `M` | **B** | Pre-existing UI component cleanups or workspace feature updates | Keep intact |
| `ui/src/app/operations/[id]/page.tsx` | `M` | **B** | Pre-existing UI component cleanups or workspace feature updates | Keep intact |
| `ui/src/app/operations/page.tsx` | `M` | **B** | Pre-existing UI component cleanups or workspace feature updates | Keep intact |
| `ui/src/app/page.tsx` | `M` | **B** | Pre-existing UI component cleanups or workspace feature updates | Keep intact |
| `ui/src/app/playground/page.tsx` | `M` | **B** | Pre-existing UI component cleanups or workspace feature updates | Keep intact |
| `ui/src/app/projects/[name]/[collection]/page.tsx` | `M` | **B** | Pre-existing UI component cleanups or workspace feature updates | Keep intact |
| `ui/src/app/projects/[name]/layout.tsx` | `M` | **B** | Pre-existing UI component cleanups or workspace feature updates | Keep intact |
| `ui/src/app/projects/[name]/page.tsx` | `M` | **B** | Pre-existing UI component cleanups or workspace feature updates | Keep intact |
| `ui/src/app/projects/page.tsx` | `M` | **B** | Pre-existing UI component cleanups or workspace feature updates | Keep intact |
| `ui/src/app/proof/page.tsx` | `M` | **B** | Pre-existing UI component cleanups or workspace feature updates | Keep intact |
| `ui/src/app/search/page.tsx` | `M` | **B** | Pre-existing UI component cleanups or workspace feature updates | Keep intact |
| `ui/src/app/snapshots/page.tsx` | `M` | **B** | Pre-existing UI component cleanups or workspace feature updates | Keep intact |
| `ui/src/components/cluster/NodeCard.tsx` | `D` | **B** | Pre-existing UI component cleanups or workspace feature updates | Keep intact |
| `ui/src/components/collections/AskTab.tsx` | `D` | **B** | Pre-existing UI component cleanups or workspace feature updates | Keep intact |
| `ui/src/components/collections/BulkInsertTab.tsx` | `D` | **B** | Pre-existing UI component cleanups or workspace feature updates | Keep intact |
| `ui/src/components/collections/CertifyTab.tsx` | `D` | **B** | Pre-existing UI component cleanups or workspace feature updates | Keep intact |
| `ui/src/components/collections/CollectionList.tsx` | `M` | **B** | Pre-existing UI component cleanups or workspace feature updates | Keep intact |
| `ui/src/components/collections/CommunityTab.tsx` | `D` | **B** | Pre-existing UI component cleanups or workspace feature updates | Keep intact |
| `ui/src/components/collections/CompliancePackTab.tsx` | `D` | **B** | Pre-existing UI component cleanups or workspace feature updates | Keep intact |
| `ui/src/components/collections/ContradictionTab.tsx` | `D` | **B** | Pre-existing UI component cleanups or workspace feature updates | Keep intact |
| `ui/src/components/collections/DiffTab.tsx` | `D` | **B** | Pre-existing UI component cleanups or workspace feature updates | Keep intact |
| `ui/src/components/collections/DocumentUploadTab.tsx` | `D` | **B** | Pre-existing UI component cleanups or workspace feature updates | Keep intact |
| `ui/src/components/collections/DocumentsTab.tsx` | `D` | **B** | Pre-existing UI component cleanups or workspace feature updates | Keep intact |
| `ui/src/components/collections/EntityExtractionTab.tsx` | `D` | **B** | Pre-existing UI component cleanups or workspace feature updates | Keep intact |
| `ui/src/components/collections/EvalTab.tsx` | `D` | **B** | Pre-existing UI component cleanups or workspace feature updates | Keep intact |
| `ui/src/components/collections/GdprTab.tsx` | `D` | **B** | Pre-existing UI component cleanups or workspace feature updates | Keep intact |
| `ui/src/components/collections/GraphTab.tsx` | `D` | **B** | Pre-existing UI component cleanups or workspace feature updates | Keep intact |
| `ui/src/components/collections/MultiSearch.tsx` | `D` | **B** | Pre-existing UI component cleanups or workspace feature updates | Keep intact |
| `ui/src/components/collections/TabShell.tsx` | `D` | **B** | Pre-existing UI component cleanups or workspace feature updates | Keep intact |
| `ui/src/components/collections/TreeRagTab.tsx` | `D` | **B** | Pre-existing UI component cleanups or workspace feature updates | Keep intact |
| `ui/src/components/collections/VerifyTab.tsx` | `D` | **B** | Pre-existing UI component cleanups or workspace feature updates | Keep intact |
| `ui/src/components/collections/VisualizeTab.tsx` | `D` | **B** | Pre-existing UI component cleanups or workspace feature updates | Keep intact |
| `ui/src/components/graph/GraphView.tsx` | `D` | **B** | Pre-existing UI component cleanups or workspace feature updates | Keep intact |
| `ui/src/components/ingestion/DocumentUploadTab.tsx` | `D` | **B** | Pre-existing UI component cleanups or workspace feature updates | Keep intact |
| `ui/src/components/layout/AppShellGate.tsx` | `M` | **B** | Pre-existing UI component cleanups or workspace feature updates | Keep intact |
| `ui/src/components/layout/Sidebar.tsx` | `M` | **B** | Pre-existing UI component cleanups or workspace feature updates | Keep intact |
| `ui/src/components/operations/OperationDetailView.tsx` | `D` | **B** | Pre-existing UI component cleanups or workspace feature updates | Keep intact |
| `ui/src/components/operations/OperationsExplorer.tsx` | `D` | **B** | Pre-existing UI component cleanups or workspace feature updates | Keep intact |
| `ui/src/components/projects/ClusterView.tsx` | `D` | **B** | Pre-existing UI component cleanups or workspace feature updates | Keep intact |
| `ui/src/components/projects/CreateProjectDialog.tsx` | `M` | **B** | Pre-existing UI component cleanups or workspace feature updates | Keep intact |
| `ui/src/components/projects/LocalRenameDialog.tsx` | `??` | **B** | Pre-existing UI component cleanups or workspace feature updates | Keep intact |
| `ui/src/components/projects/MetricsView.tsx` | `D` | **B** | Pre-existing UI component cleanups or workspace feature updates | Keep intact |
| `ui/src/components/projects/PlaygroundView.tsx` | `D` | **B** | Pre-existing UI component cleanups or workspace feature updates | Keep intact |
| `ui/src/components/projects/ProjectCard.tsx` | `??` | **B** | Pre-existing UI component cleanups or workspace feature updates | Keep intact |
| `ui/src/components/projects/SnapshotsView.tsx` | `D` | **B** | Pre-existing UI component cleanups or workspace feature updates | Keep intact |
| `ui/src/components/proof/ProofHash.tsx` | `D` | **B** | Pre-existing UI component cleanups or workspace feature updates | Keep intact |
| `ui/src/components/settings/SettingsModal.tsx` | `M` | **B** | Pre-existing UI component cleanups or workspace feature updates | Keep intact |
| `ui/src/components/snapshots/` | `??` | **B** | Pre-existing UI component cleanups or workspace feature updates | Keep intact |
| `ui/src/lib/cloud-runtime/` | `??` | **B** | Pre-existing UI component cleanups or workspace feature updates | Keep intact |
| `ui/src/lib/hooks/useGraph.ts` | `D` | **B** | Pre-existing UI component cleanups or workspace feature updates | Keep intact |
| `ui/src/lib/hooks/useMetrics.ts` | `M` | **B** | Pre-existing UI component cleanups or workspace feature updates | Keep intact |
| `ui/src/lib/local-runtime/` | `??` | **B** | Pre-existing UI component cleanups or workspace feature updates | Keep intact |
| `ui/src/lib/server/daemon.ts` | `M` | **B** | Pre-existing UI component cleanups or workspace feature updates | Keep intact |
| `ui/src/lib/valori-client.ts` | `D` | **B** | Pre-existing UI component cleanups or workspace feature updates | Keep intact |
| `ui/src/utils/supabase/dal.ts` | `??` | **B** | Pre-existing UI component cleanups or workspace feature updates | Keep intact |
| `ui/studio/` | `??` | **B** | Pre-existing UI component cleanups or workspace feature updates | Keep intact |
| `ui/tsconfig.json` | `M` | **B** | Pre-existing UI component cleanups or workspace feature updates | Keep intact |
| `crates/valori-consensus/Cargo.toml` | `M` | **C** | Storage / State / Consensus support dependency | Verify build & tests |
| `crates/valori-consensus/README.md` | `M` | **C** | Storage / State / Consensus support dependency | Verify build & tests |
| `crates/valori-consensus/src/state_machine.rs` | `M` | **C** | Storage / State / Consensus support dependency | Verify build & tests |
| `crates/valori-consensus/tests/state_machine.rs` | `M` | **C** | Storage / State / Consensus support dependency | Verify build & tests |
| `crates/valori-engine/Cargo.toml` | `M` | **C** | Engine error mapping or standalone request_id dedup storage | Verify engine error & dedup invariants |
| `crates/valori-engine/README.md` | `M` | **C** | Engine error mapping or standalone request_id dedup storage | Verify engine error & dedup invariants |
| `crates/valori-engine/src/config.rs` | `M` | **C** | Engine error mapping or standalone request_id dedup storage | Verify engine error & dedup invariants |
| `crates/valori-engine/src/lib.rs` | `M` | **C** | Engine error mapping or standalone request_id dedup storage | Verify engine error & dedup invariants |
| `crates/valori-kernel/README.md` | `M` | **C** | Kernel internal state/event/snapshot implementation | Isolate kernel internals |
| `crates/valori-kernel/src/state/kernel.rs` | `M` | **C** | Kernel internal state/event/snapshot implementation | Isolate kernel internals |
| `crates/valori-kernel/tests/format.rs` | `M` | **C** | Kernel internal state/event/snapshot implementation | Isolate kernel internals |
| `crates/valori-kernel/tests/state_machine.rs` | `M` | **C** | Kernel internal state/event/snapshot implementation | Isolate kernel internals |
| `crates/valori-node/Cargo.toml` | `M` | **C** | Valori node server dependency | Keep intact |
| `crates/valori-node/README.md` | `M` | **C** | Valori node server dependency | Keep intact |
| `crates/valori-node/examples/crash_recovery_demo.rs` | `M` | **C** | Valori node server dependency | Keep intact |
| `crates/valori-node/src/api_keys.rs` | `M` | **C** | Valori node server dependency | Keep intact |
| `crates/valori-node/src/bin/` | `??` | **C** | Valori node server dependency | Keep intact |
| `crates/valori-node/src/capabilities.rs` | `M` | **C** | Valori node server dependency | Keep intact |
| `crates/valori-node/src/cluster.rs` | `M` | **C** | Valori node server dependency | Keep intact |
| `crates/valori-node/src/engine.rs` | `M` | **C** | Valori node server dependency | Keep intact |
| `crates/valori-node/src/ingest.rs` | `M` | **C** | Valori node server dependency | Keep intact |
| `crates/valori-node/src/kernel_writer.rs` | `M` | **C** | Valori node server dependency | Keep intact |
| `crates/valori-node/src/lib.rs` | `M` | **C** | Valori node server dependency | Keep intact |
| `crates/valori-node/src/telemetry.rs` | `M` | **C** | Valori node server dependency | Keep intact |
| `crates/valori-node/tests/api_as_of.rs` | `M` | **C** | Node integration test updated for API-2 error response or request shape | Verify test assertions |
| `crates/valori-node/tests/api_batch_idempotency.rs` | `M` | **C** | Node integration test updated for API-2 error response or request shape | Verify test assertions |
| `crates/valori-node/tests/api_batch_ingest.rs` | `M` | **C** | Node integration test updated for API-2 error response or request shape | Verify test assertions |
| `crates/valori-node/tests/api_crypto_shred.rs` | `M` | **C** | Node integration test updated for API-2 error response or request shape | Verify test assertions |
| `crates/valori-node/tests/api_decay.rs` | `M` | **C** | Node integration test updated for API-2 error response or request shape | Verify test assertions |
| `crates/valori-node/tests/api_graph_cascade_delete.rs` | `??` | **C** | Node integration test updated for API-2 error response or request shape | Verify test assertions |
| `crates/valori-node/tests/api_graph_namespace_isolation.rs` | `??` | **C** | Node integration test updated for API-2 error response or request shape | Verify test assertions |
| `crates/valori-node/tests/api_graph_query.rs` | `??` | **C** | Node integration test updated for API-2 error response or request shape | Verify test assertions |
| `crates/valori-node/tests/api_graphrag.rs` | `M` | **C** | Node integration test updated for API-2 error response or request shape | Verify test assertions |
| `crates/valori-node/tests/api_keys.rs` | `M` | **C** | Node integration test updated for API-2 error response or request shape | Verify test assertions |
| `crates/valori-node/tests/api_misc.rs` | `M` | **C** | Node integration test updated for API-2 error response or request shape | Verify test assertions |
| `crates/valori-node/tests/api_object_store.rs` | `M` | **C** | Node integration test updated for API-2 error response or request shape | Verify test assertions |
| `crates/valori-node/tests/api_proof.rs` | `M` | **C** | Node integration test updated for API-2 error response or request shape | Verify test assertions |
| `crates/valori-node/tests/api_replication.rs` | `M` | **C** | Node integration test updated for API-2 error response or request shape | Verify test assertions |
| `crates/valori-node/tests/api_tree.rs` | `M` | **C** | Node integration test updated for API-2 error response or request shape | Verify test assertions |
| `crates/valori-node/tests/cluster_ann_hardening.rs` | `??` | **C** | Node integration test updated for API-2 error response or request shape | Verify test assertions |
| `crates/valori-node/tests/cluster_api.rs` | `M` | **C** | Node integration test updated for API-2 error response or request shape | Verify test assertions |
| `crates/valori-node/tests/cluster_boot.rs` | `M` | **C** | Node integration test updated for API-2 error response or request shape | Verify test assertions |
| `crates/valori-node/tests/cluster_data_plane.rs` | `M` | **C** | Node integration test updated for API-2 error response or request shape | Verify test assertions |
| `crates/valori-node/tests/cluster_graph_aware_reranking.rs` | `??` | **C** | Node integration test updated for API-2 error response or request shape | Verify test assertions |
| `crates/valori-node/tests/cluster_graph_cascade_delete.rs` | `??` | **C** | Node integration test updated for API-2 error response or request shape | Verify test assertions |
| `crates/valori-node/tests/cluster_graph_namespace_isolation.rs` | `??` | **C** | Node integration test updated for API-2 error response or request shape | Verify test assertions |
| `crates/valori-node/tests/cluster_namespaces.rs` | `M` | **C** | Node integration test updated for API-2 error response or request shape | Verify test assertions |
| `crates/valori-node/tests/cluster_read_index.rs` | `M` | **C** | Node integration test updated for API-2 error response or request shape | Verify test assertions |
| `crates/valori-node/tests/cluster_search_namespace_isolation.rs` | `??` | **C** | Node integration test updated for API-2 error response or request shape | Verify test assertions |
| `crates/valori-node/tests/collections.rs` | `M` | **C** | Node integration test updated for API-2 error response or request shape | Verify test assertions |
| `crates/valori-node/tests/dependency_direction.rs` | `M` | **C** | Node integration test updated for API-2 error response or request shape | Verify test assertions |
| `crates/valori-node/tests/deterministic_edge_tests.rs` | `M` | **C** | Node integration test updated for API-2 error response or request shape | Verify test assertions |
| `crates/valori-node/tests/dr_disaster_recovery.rs` | `M` | **C** | Node integration test updated for API-2 error response or request shape | Verify test assertions |
| `crates/valori-node/tests/e2e_proof.rs` | `M` | **C** | Node integration test updated for API-2 error response or request shape | Verify test assertions |
| `crates/valori-node/tests/e2e_recovery.rs` | `M` | **C** | Node integration test updated for API-2 error response or request shape | Verify test assertions |
| `crates/valori-node/tests/engine_snapshot_roundtrip.rs` | `M` | **C** | Node integration test updated for API-2 error response or request shape | Verify test assertions |
| `crates/valori-node/tests/graph_aware_reranking.rs` | `??` | **C** | Node integration test updated for API-2 error response or request shape | Verify test assertions |
| `crates/valori-node/tests/graph_cascade.rs` | `M` | **C** | Node integration test updated for API-2 error response or request shape | Verify test assertions |
| `crates/valori-node/tests/graph_cascade_delete.rs` | `??` | **C** | Node integration test updated for API-2 error response or request shape | Verify test assertions |
| `crates/valori-node/tests/graph_query_restart_recovery.rs` | `??` | **C** | Node integration test updated for API-2 error response or request shape | Verify test assertions |
| `crates/valori-node/tests/health_metrics.rs` | `M` | **C** | Node integration test updated for API-2 error response or request shape | Verify test assertions |
| `crates/valori-node/tests/hnsw_tests.rs` | `M` | **C** | Node integration test updated for API-2 error response or request shape | Verify test assertions |
| `crates/valori-node/tests/index_artifact_persistence.rs` | `??` | **C** | Node integration test updated for API-2 error response or request shape | Verify test assertions |
| `crates/valori-node/tests/index_lifecycle.rs` | `??` | **C** | Node integration test updated for API-2 error response or request shape | Verify test assertions |
| `crates/valori-node/tests/integration_tests.rs` | `M` | **C** | Node integration test updated for API-2 error response or request shape | Verify test assertions |
| `crates/valori-node/tests/ivf_recall.rs` | `M` | **C** | Node integration test updated for API-2 error response or request shape | Verify test assertions |
| `crates/valori-node/tests/memory_search_parity.rs` | `M` | **C** | Node integration test updated for API-2 error response or request shape | Verify test assertions |
| `crates/valori-node/tests/multi_arch_determinism.rs` | `M` | **C** | Node integration test updated for API-2 error response or request shape | Verify test assertions |
| `crates/valori-node/tests/multi_collection_search.rs` | `??` | **C** | Node integration test updated for API-2 error response or request shape | Verify test assertions |
| `crates/valori-node/tests/persistence_index_tests.rs` | `M` | **C** | Node integration test updated for API-2 error response or request shape | Verify test assertions |
| `crates/valori-node/tests/persistence_tests.rs` | `M` | **C** | Node integration test updated for API-2 error response or request shape | Verify test assertions |
| `crates/valori-node/tests/planner_parity.rs` | `M` | **C** | Node integration test updated for API-2 error response or request shape | Verify test assertions |
| `crates/valori-node/tests/proof_e2e_tests.rs` | `M` | **C** | Node integration test updated for API-2 error response or request shape | Verify test assertions |
| `crates/valori-node/tests/replication_bootstrap.rs` | `M` | **C** | Node integration test updated for API-2 error response or request shape | Verify test assertions |
| `crates/valori-node/tests/replication_cluster.rs` | `M` | **C** | Node integration test updated for API-2 error response or request shape | Verify test assertions |
| `crates/valori-node/tests/replication_divergence.rs` | `M` | **C** | Node integration test updated for API-2 error response or request shape | Verify test assertions |
| `crates/valori-node/tests/search_k_bounds.rs` | `M` | **C** | Node integration test updated for API-2 error response or request shape | Verify test assertions |
| `crates/valori-node/tests/usage_endpoint_tests.rs` | `M` | **C** | Node integration test updated for API-2 error response or request shape | Verify test assertions |
| `crates/valori-node/tests/vector_graph_retrieval.rs` | `??` | **C** | Node integration test updated for API-2 error response or request shape | Verify test assertions |
| `crates/valori-state/Cargo.toml` | `M` | **C** | Storage / State / Consensus support dependency | Verify build & tests |
| `crates/valori-state/README.md` | `M` | **C** | Storage / State / Consensus support dependency | Verify build & tests |
| `crates/valori-state/src/error.rs` | `M` | **C** | Storage / State / Consensus support dependency | Verify build & tests |
| `crates/valori-state/src/lib.rs` | `M` | **C** | Storage / State / Consensus support dependency | Verify build & tests |
| `crates/valori-state/tests/fixtures/event_log_inserts.toml` | `M` | **C** | Storage / State / Consensus support dependency | Verify build & tests |
| `crates/valori-state/tests/fixtures/event_log_namespace.toml` | `M` | **C** | Storage / State / Consensus support dependency | Verify build & tests |
| `crates/valori-storage/Cargo.toml` | `M` | **C** | Storage / State / Consensus support dependency | Verify build & tests |
| `crates/valori-storage/README.md` | `M` | **C** | Storage / State / Consensus support dependency | Verify build & tests |
| `crates/valori-storage/src/events/event_commit.rs` | `M` | **C** | Storage / State / Consensus support dependency | Verify build & tests |
| `crates/valori-storage/src/events/event_journal.rs` | `M` | **C** | Storage / State / Consensus support dependency | Verify build & tests |
| `crates/valori-storage/src/events/event_log.rs` | `M` | **C** | Storage / State / Consensus support dependency | Verify build & tests |
| `crates/valori-storage/src/events/event_replay.rs` | `M` | **C** | Storage / State / Consensus support dependency | Verify build & tests |
| `crates/valori-storage/src/lib.rs` | `M` | **C** | Storage / State / Consensus support dependency | Verify build & tests |
| `crates/valori-storage/src/provider/` | `??` | **C** | Storage / State / Consensus support dependency | Verify build & tests |
| `crates/valori-storage/tests/fixtures/wal_v1_inserts.hash` | `M` | **C** | Storage / State / Consensus support dependency | Verify build & tests |
| `crates/valori-storage/tests/fixtures/wal_v1_namespace.hash` | `M` | **C** | Storage / State / Consensus support dependency | Verify build & tests |
| `python/tests/test_index_lifecycle.py` | `??` | **C** | Python SDK test or helper | Verify SDK test suite |
| `crates/valori-engine/src/index_manager.rs` | `??` | **D** | Pre-existing/unfinished platform feature (Index/Snapshot v8/Graph/Storage manifest) | Isolate without modification or deletion |
| `crates/valori-kernel/src/error.rs` | `M` | **D** | Kernel internal state/event/snapshot implementation | Isolate kernel internals |
| `crates/valori-kernel/src/event.rs` | `M` | **D** | Kernel internal state/event/snapshot implementation | Isolate kernel internals |
| `crates/valori-kernel/src/index/mod.rs` | `M` | **D** | Kernel internal state/event/snapshot implementation | Isolate kernel internals |
| `crates/valori-kernel/src/replay_events.rs` | `D` | **D** | Kernel internal state/event/snapshot implementation | Isolate kernel internals |
| `crates/valori-kernel/src/snapshot/blake3.rs` | `M` | **D** | Kernel internal state/event/snapshot implementation | Isolate kernel internals |
| `crates/valori-kernel/src/snapshot/decode.rs` | `M` | **D** | Kernel internal state/event/snapshot implementation | Isolate kernel internals |
| `crates/valori-kernel/src/snapshot/encode.rs` | `M` | **D** | Kernel internal state/event/snapshot implementation | Isolate kernel internals |
| `crates/valori-kernel/tests/fixtures/snapshot_v7_empty.hash` | `M` | **D** | Kernel internal state/event/snapshot implementation | Isolate kernel internals |
| `crates/valori-kernel/tests/fixtures/snapshot_v7_multi.hash` | `M` | **D** | Kernel internal state/event/snapshot implementation | Isolate kernel internals |
| `crates/valori-kernel/tests/fixtures/snapshot_v7_single.hash` | `M` | **D** | Kernel internal state/event/snapshot implementation | Isolate kernel internals |
| `crates/valori-kernel/tests/fixtures/snapshot_v8_multi_collections.bin` | `??` | **D** | Pre-existing/unfinished platform feature (Index/Snapshot v8/Graph/Storage manifest) | Isolate without modification or deletion |
| `crates/valori-kernel/tests/fixtures/snapshot_v8_multi_collections.hash` | `??` | **D** | Pre-existing/unfinished platform feature (Index/Snapshot v8/Graph/Storage manifest) | Isolate without modification or deletion |
| `crates/valori-kernel/tests/graph_g01_invariants.rs` | `??` | **D** | Pre-existing/unfinished platform feature (Index/Snapshot v8/Graph/Storage manifest) | Isolate without modification or deletion |
| `crates/valori-kernel/tests/snapshot_compat.rs` | `M` | **D** | Kernel internal state/event/snapshot implementation | Isolate kernel internals |
| `crates/valori-kernel/tests/snapshot_roundtrip.rs` | `M` | **D** | Kernel internal state/event/snapshot implementation | Isolate kernel internals |
| `crates/valori-search/src/graph_rerank.rs` | `??` | **D** | Pre-existing/unfinished platform feature (Index/Snapshot v8/Graph/Storage manifest) | Isolate without modification or deletion |
| `crates/valori-state/src/collection_bootstrap.rs` | `??` | **D** | Pre-existing/unfinished platform feature (Index/Snapshot v8/Graph/Storage manifest) | Isolate without modification or deletion |
| `crates/valori-storage/src/collection_manifest.rs` | `??` | **D** | Pre-existing/unfinished platform feature (Index/Snapshot v8/Graph/Storage manifest) | Isolate without modification or deletion |
| `crates/valori-storage/src/collection_snapshot.rs` | `??` | **D** | Pre-existing/unfinished platform feature (Index/Snapshot v8/Graph/Storage manifest) | Isolate without modification or deletion |
| `crates/valori-storage/src/project_manifest.rs` | `??` | **D** | Pre-existing/unfinished platform feature (Index/Snapshot v8/Graph/Storage manifest) | Isolate without modification or deletion |
| `CHANGELOG.md` | `M` | **E** | Documentation or generated contract artifact | Keep & audit integrity |
| `README.md` | `M` | **E** | Documentation or generated contract artifact | Keep & audit integrity |
| `api/` | `??` | **E** | Documentation or generated contract artifact | Keep & audit integrity |
| `docs/api-reference.md` | `M` | **E** | Documentation or generated contract artifact | Keep & audit integrity |
| `docs/api/` | `??` | **E** | Documentation or generated contract artifact | Keep & audit integrity |
| `docs/architecture/shared-studio.md` | `??` | **E** | Documentation or generated contract artifact | Keep & audit integrity |
| `docs/phases/README.md` | `M` | **E** | Documentation or generated contract artifact | Keep & audit integrity |
| `docs/phases/phase-2.4-storage-coherence.md` | `??` | **E** | Documentation or generated contract artifact | Keep & audit integrity |
| `docs/phases/phase-3.0-remove-process-wide-vector-config.md` | `??` | **E** | Documentation or generated contract artifact | Keep & audit integrity |
| `docs/phases/phase-3.2-eliminate-implicit-unconfigured-collection.md` | `??` | **E** | Documentation or generated contract artifact | Keep & audit integrity |
| `docs/phases/phase-3.3-zero-collection-projects.md` | `??` | **E** | Documentation or generated contract artifact | Keep & audit integrity |
| `docs/phases/phase-4-index-lifecycle.md` | `??` | **E** | Documentation or generated contract artifact | Keep & audit integrity |
| `docs/phases/phase-4.1-index-lifecycle-hardening.md` | `??` | **E** | Documentation or generated contract artifact | Keep & audit integrity |
| `docs/phases/phase-4.2-index-lifecycle-ui.md` | `??` | **E** | Documentation or generated contract artifact | Keep & audit integrity |
| `docs/phases/phase-4.3-cluster-ann.md` | `??` | **E** | Documentation or generated contract artifact | Keep & audit integrity |
| `docs/phases/phase-4.4-cluster-ann-hardening.md` | `??` | **E** | Documentation or generated contract artifact | Keep & audit integrity |
| `docs/phases/phase-5-cross-collection-search.md` | `??` | **E** | Documentation or generated contract artifact | Keep & audit integrity |
| `docs/phases/phase-5.1-graph-query-audit-and-metrics.md` | `??` | **E** | Documentation or generated contract artifact | Keep & audit integrity |
| `docs/phases/phase-5.2-graphrag-query-orchestration.md` | `??` | **E** | Documentation or generated contract artifact | Keep & audit integrity |
| `docs/phases/phase-5.3-graphrag-semantic-hardening.md` | `??` | **E** | Documentation or generated contract artifact | Keep & audit integrity |
| `docs/phases/phase-5.4-graphrag-reranking-budgets.md` | `??` | **E** | Documentation or generated contract artifact | Keep & audit integrity |
| `docs/phases/phase-G1.3.1-record-graph-cascade-fix.md` | `??` | **E** | Documentation or generated contract artifact | Keep & audit integrity |
| `docs/phases/phase-G1.4.1-graph-aware-reranking.md` | `??` | **E** | Documentation or generated contract artifact | Keep & audit integrity |
| `docs/phases/phase-G1.4.2-cluster-search-namespace-isolation.md` | `??` | **E** | Documentation or generated contract artifact | Keep & audit integrity |
| `docs/phases/phase-api-1-contract-audit.md` | `??` | **E** | Documentation or generated contract artifact | Keep & audit integrity |
| `docs/phases/phase-api-contract-2-convergence.md` | `??` | **E** | Documentation or generated contract artifact | Keep & audit integrity |
| `docs/phases/phase-collection-scoped-vector-config.md` | `??` | **E** | Documentation or generated contract artifact | Keep & audit integrity |
| `docs/reviews/graph-g0-architecture-audit.md` | `??` | **E** | Documentation or generated contract artifact | Keep & audit integrity |
| `docs/reviews/graph-g0.1-determinism-state-integrity.md` | `??` | **E** | Documentation or generated contract artifact | Keep & audit integrity |
| `docs/reviews/graph-g0.2-canonical-state-hash-commitment.md` | `??` | **E** | Documentation or generated contract artifact | Keep & audit integrity |
| `docs/reviews/graph-g1.0-evolution-contract.md` | `??` | **E** | Documentation or generated contract artifact | Keep & audit integrity |
| `docs/reviews/graph-g1.1-query-primitives.md` | `??` | **E** | Documentation or generated contract artifact | Keep & audit integrity |
| `docs/reviews/graph-g1.1.1-graph-read-namespace-isolation.md` | `??` | **E** | Documentation or generated contract artifact | Keep & audit integrity |
| `docs/reviews/graph-g1.2-traversal-performance.md` | `??` | **E** | Documentation or generated contract artifact | Keep & audit integrity |
| `docs/reviews/graph-g1.3-vector-graph-retrieval.md` | `??` | **E** | Documentation or generated contract artifact | Keep & audit integrity |
| `docs/reviews/graph-g1.3.1-record-graph-cascade-semantics.md` | `??` | **E** | Documentation or generated contract artifact | Keep & audit integrity |
| `docs/reviews/graph-g1.4-hybrid-retrieval-design.md` | `??` | **E** | Documentation or generated contract artifact | Keep & audit integrity |
| `docs/reviews/graph-g1.4.1-graph-aware-reranking-design.md` | `??` | **E** | Documentation or generated contract artifact | Keep & audit integrity |
| `docs/reviews/graph-g1.4.3-cluster-index-capability-audit.md` | `??` | **E** | Documentation or generated contract artifact | Keep & audit integrity |
| `docs/reviews/project-collection-g2.0-domain-model.md` | `??` | **E** | Documentation or generated contract artifact | Keep & audit integrity |
| `docs/reviews/project-collection-g2.0.1-drop-namespace-semantics.md` | `??` | **E** | Documentation or generated contract artifact | Keep & audit integrity |
| `docs/reviews/project-collection-lifecycle-audit.md` | `??` | **E** | Documentation or generated contract artifact | Keep & audit integrity |
| `docs/reviews/recovery-hnsw-startup-breakdown.md` | `??` | **E** | Documentation or generated contract artifact | Keep & audit integrity |
| `ui/api-types/` | `??` | **E** | Documentation or generated contract artifact | Keep & audit integrity |

---

## Special Boundary Review: Index / Snapshot / Kernel Files

The following 11 suspicious / pre-existing platform files were evaluated against the 6 boundary questions to ensure they are properly isolated and do not corrupt the API-2 contract:

| File | Context | 1. Before API-2? | 2. Used by API-2? | 3. Imported by API-2? | 4. Build Required? | 5. Test Required? | 6. Changes Public API? | Resolution & Action |
|------|---------|------------------|-------------------|-----------------------|--------------------|-------------------|------------------------|---------------------|
| `crates/valori-engine/src/index_manager.rs` | Untracked file implementing asynchronous background index lifecycle manager. | False | False | False | True | False | False | **Isolate. Required for cargo build if index_lifecycle feature is referenced, but not part of public API v1 endpoints.** |
| `crates/valori-kernel/tests/fixtures/snapshot_v8_multi_collections.bin` | Untracked snapshot v8 binary fixture for multi-collection snapshot test. | False | False | False | False | True | False | **Isolate fixture. Does not affect HTTP API v1 contract.** |
| `crates/valori-kernel/tests/fixtures/snapshot_v8_multi_collections.hash` | Untracked snapshot v8 BLAKE3 hash fixture. | False | False | False | False | True | False | **Isolate fixture. Internal storage binary compatibility.** |
| `crates/valori-kernel/src/snapshot/decode.rs` | Modified kernel V8 snapshot decoder supporting multi-namespace headers. | False | False | True | True | True | False | **Isolate kernel internals. Required for kernel snapshot compatibility.** |
| `crates/valori-kernel/src/snapshot/encode.rs` | Modified kernel V8 snapshot encoder supporting multi-namespace headers. | False | False | True | True | True | False | **Isolate kernel internals. Required for kernel snapshot compatibility.** |
| `crates/valori-kernel/src/state/kernel.rs` | Modified KernelState for snapshot V8 and multi-namespace linked list traversal. | False | False | True | True | True | False | **Isolate kernel internals. Internal deterministic state machine.** |
| `crates/valori-kernel/tests/graph_g01_invariants.rs` | Untracked kernel test verifying Graph G0.1 determinism and state integrity invariants. | False | False | False | True | True | False | **Isolate test. Internal kernel verification.** |
| `crates/valori-search/src/graph_rerank.rs` | Untracked graph-aware re-ranking implementation for hybrid GraphRAG search. | False | False | False | True | True | False | **Isolate module. Internal graph-aware re-ranking vector search helper.** |
| `crates/valori-storage/src/collection_manifest.rs` | Untracked storage manifest module for durable Collection metadata. | False | False | False | True | True | False | **Isolate module. Durable collection metadata persistence.** |
| `crates/valori-storage/src/project_manifest.rs` | Untracked storage manifest module for durable Project metadata. | False | False | False | True | True | False | **Isolate module. Durable project metadata persistence.** |
| `crates/valori-state/src/collection_bootstrap.rs` | Untracked collection bootstrap orchestrator. | False | False | False | True | True | False | **Isolate module. State recovery helper.** |
