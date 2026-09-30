# Changelog

All notable changes to `@valori/studio` are documented here. Versioning
policy is documented in `README.md`.

## 0.3.1 — Republish of 0.3.0

`0.3.0`'s published tarball 404s on the npm registry (`npm install`
fails, README does not render) despite valid registry metadata — a bad
upload, not a code problem. npm does not allow overwriting a published
version, so this is a content-identical republish. See 0.3.0 below for
the actual changes.

## 0.3.0 — Assertions tab + Cloud/Desktop parity fixes

The last published version (registry `0.2.2`) predates this entry; the
0.3.0/0.3.1 entries below it describe work that had already shipped inside
0.2.x without ever being logged here, and the numbering does not line up
with npm's actual `0.1.0 → 0.2.0 → 0.2.1 → 0.2.2` publish history. This
entry covers only what changed since the commit `0.2.2` was actually
published from.

**New tab component:**
- `AssertionsTab` — verify two structured claims (subject/predicate/object,
  optional negation/time-scope) against `POST /v1/assertions/verify` and
  look up a stored receipt via `GET /v1/assertions/verification/{id}`.
  Wired into `ToolsWorkspace`'s Analyze tabs as "Assertions".

**Breaking (allowed under 0.x semver, no known in-repo consumer):**
- Removed the `useSearch` hook and its `SearchQuery`/`SearchState` types
  from the public export surface (`src/index.ts`) — the underlying hook
  file was deleted. Nothing in Cloud Web, Desktop Local, or Desktop Cloud
  imports `useSearch` from `@valori/studio`; each has its own local search
  hook. An external consumer of the published package that used it would
  need to bring their own replacement.

**Fixed:**
- `main`/`module`/`types`/`exports` in `package.json` now point at
  `./dist/studio/src/index.js` (matching `tsconfig.json`'s `rootDir: ".."`
  output layout) instead of `./dist/index.js`, which did not match what
  the build actually produced.
- `ToolsWorkspace`: `useCollectionIndex` import switched to a relative path
  (works around a resolution issue with the `@/` alias in this build), and
  the collection-dimension lookup no longer assumes `dimension` exists on
  the narrower ref type used here.

## Cluster ANN support (Phase 4.3)

_Already present in the current published version — this entry predates real version numbering and never had one; kept for phase history._

Patch. Phase 4.3 removes the cluster-mode ANN limitation at the backend level.

**Changed:**
- `IndexLifecycleTab` — removed the amber "not available in cluster mode" banner
  that appeared when a write action returned 501. The backend now supports HNSW/IVF/BQ
  builds in cluster mode (Phase 4.3); the 501 path is unreachable and the banner was
  stale. Error messages from any other unexpected backend failure still surface inline
  via the existing `actionError` display.

## Index Lifecycle UI (Phase 4.2)

_Already present in the current published version — this entry predates real version numbering and never had one; kept for phase history._

Additive release. All 0.2.0 exports and props remain unchanged.

**New hook:**
- `useCollectionIndex(projectId, namespace)` — polls `GET /v1/namespaces/{name}/index`
  for live per-collection index lifecycle state. Polls at 3 s during transient states
  (`building`, `ready`); stops polling for terminal states (`none`, `active`, `failed`).
  Revalidates on window focus.

**New tab component:**
- `IndexLifecycleTab` — full collection index lifecycle UI: all 5 states (none / building /
  ready / active / failed), Create / Change / Remove inline action panels with HNSW and IVF
  parameter inputs, spinner-free live polling, cluster-501 detection.

**`ToolsWorkspace` changes:**
- `ANALYZE_TABS` now includes `{ value: 'index', label: 'Index' }` as the first entry.
  Renders `IndexLifecycleTab` for the selected collection.
- `CollectionHeader` now shows collection-specific live index status from `useCollectionIndex`
  (previously used the project-wide `/health` index field, which is not per-collection).
- "View details" button now navigates to the Index tab (previously navigated to Info).

## 0.2.0 — shared feature reconciliation (unpublished)

Backward-compatible MINOR release. Everything below is additive — no
existing 0.1.0 export, required prop, or capability behavior changed.

**New public export:**
- `OperationDetailView` — extracted from Desktop Local/Cloud's shared
  dual-mode component and Cloud Web's near-identical separate copy (the
  investigation found only route-string differences between them). Uses
  `useTransport()` and a host-supplied `backHref`, same pattern as every
  other view. Its "Execution Explorer" tab is host-injectable via an
  optional `renderExecution` render-prop rather than ported wholesale,
  since the full graph visualization depends on `@xyflow/react`, which
  Studio does not declare as a dependency (not authorized to add this
  release) — omitting the prop simply hides that one tab.

**New optional props (all backward-compatible, all off/hidden by default):**
- `GraphView`: `embedded?: boolean` — hides the redundant node/doc/chunk
  count line when mounted inside a host's own tab/collection chrome;
  rendering, the Tree/Canvas toggle, and data/loading/error behavior are
  unchanged either way.
- `ProofView`: `receiptCard?: ReactNode`, `exportActions?: ReactNode` —
  host-supplied slots for genuine shared product functionality (a receipt
  panel, export controls) the investigation found was mistakenly trimmed
  during the original Cloud-sourced extraction, not actually Local-only.
- `SnapshotsView`: `capabilities?: StudioCapabilities`,
  `localFilesPanel?: ReactNode` — reuses the existing `localFilesystem`
  capability (no new flag) to gate an optional host-supplied local-files
  panel; multi-project switching stays entirely host-level, not in Studio.
- `ToolsWorkspace` / `DocumentUploadTab`: `capabilities?: StudioCapabilities`,
  `settingsHref?: string` — see the new capability below.

**New capability field:**
- `StudioCapabilities.clientEmbeddingFallback?: boolean` (optional,
  default unset/off) — gates `DocumentUploadTab`'s client-driven fallback
  form fields (embedding credentials, chunk config, contextual enrichment)
  for a host whose own ingest route can chunk+embed+insert itself when the
  node has no server embed provider configured.

## 0.1.0 — initial internal release (unpublished)

Extracted from `Valori-Kernel/ui` (Phase C) and hardened for cross-repo
distribution (Phase F-prep). Not yet published to a registry — consumed
today only via npm workspace (`Valori-Kernel/ui`, both Desktop Local and
Desktop Cloud).

**Included:**
- Core product views: `ClusterView`, `MetricsView`, `SnapshotsView`,
  `PlaygroundView`, `GraphView`, `OperationsExplorer`, `ProofView`.
- `ToolsWorkspace` and its 17 tabs (Search, Upload, Bulk Insert, Visualize,
  Ask, Documents, Tree-RAG, Communities, Entity Extract, Eval, Diff,
  Contradictions, Verify, Certify, GDPR, Compliance, Info).
- Runtime contract: `Transport`, `CredentialStore`, `StudioCapabilities`,
  `StudioRuntime`, `ProjectRef`, `StudioProvider` + hooks.
- `PROJECT_FEATURE_NAV` shared navigation data.
- Product hooks: `useHealth`, `useCluster`, `useProof`, `useGraph`,
  `useNodeEdges`, `useSearch`, `useCollections`, `useEmbeddingConfig`,
  `useLLMConfig`.
- Zero Tauri, Supabase, or Next.js server dependencies — verified by a
  full source + dependency-tree scan (Phase F-prep, §7).
