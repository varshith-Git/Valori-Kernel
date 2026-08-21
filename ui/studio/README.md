# @valori/studio

Shared Valori product UI — the normal customer-facing feature set (projects,
collections, search, metrics, cluster, graph, operations, proof, snapshots,
tools, playground). One implementation, consumed by every host application:
Desktop Local, Desktop Cloud, and (future) Cloud Web.

This package contains **no** Tauri, Supabase, Next.js server APIs, or Cloud
control-plane code. A host injects a `Transport`, an optional
`CredentialStore`, and `StudioCapabilities`; Studio never knows which host
it's running inside.

## Install

Within the `Valori-Kernel` monorepo this is consumed as an npm workspace
member (`"@valori/studio": "*"` in the host's `package.json`, no registry
involved). For a host outside this repository, install the packed/published
tarball and supply `react`/`react-dom` yourself — both are peer
dependencies, never bundled or duplicated by this package.

```bash
npm install @valori/studio react react-dom
```

## Usage

```tsx
import { StudioProvider, MetricsView, type Transport } from "@valori/studio";

const transport: Transport = {
  path: (projectId, subpath) => `/api/your-host-prefix/${projectId}${subpath}`,
};

function App() {
  return (
    <StudioProvider runtime={{ transport, capabilities: { localFilesystem: false, multiCollectionPicker: true, osKeychain: false } }}>
      <MetricsView projectId="proj_123" capabilities={{ localFilesystem: false }} />
    </StudioProvider>
  );
}
```

Everything is imported from the package root (`@valori/studio`) — there are
no intentional deep import paths, and `src/` is not published.

## Public API

**Runtime contract** — the interfaces a host must implement:
`Transport`, `CredentialStore`, `StudioCapabilities`, `StudioRuntime`,
`ProjectRef`, plus the provider/hooks that wire them into components:
`StudioProvider`, `useTransport`, `useCredentialStore`, `useCapabilities`.

**Navigation**: `PROJECT_FEATURE_NAV` — feature registry data (not routes);
each host renders its own nav shell from it.

**Core product views**: `ClusterView`, `MetricsView`, `SnapshotsView`,
`PlaygroundView`, `GraphView` (accepts `embedded?: boolean` since 0.2.0 —
defaults to standalone-page chrome, unchanged), `OperationsExplorer`,
`OperationDetailView` (new in 0.2.0 — accepts a `renderExecution` render-prop
for a host-supplied Execution Explorer tab; omit to hide that one tab),
`ProofView` (accepts `receiptCard?`/`exportActions?` slots since 0.2.0 for
host-supplied receipt/export UI — omit for unchanged behavior).

**Tools**: `ToolsWorkspace` plus its individual tabs (`MultiSearch`,
`DocumentUploadTab`, `DocumentsTab`, `BulkInsertTab`, `TreeRagTab`,
`CommunityTab`, `EntityExtractionTab`, `DiffTab`, `ContradictionTab`,
`VerifyTab`, `CompliancePackTab`, `EvalTab`, `CertifyTab`, `GdprTab`,
`VisualizeTab`, `AskTab`, `TabShell`) — exported individually so a host can
compose a custom tab layout instead of the full `ToolsWorkspace` if needed.

**Hooks**: `useHealth`, `useCluster`, `useProof`, `useGraph`,
`useNodeEdges`, `useSearch`, `useCollections`, `useEmbeddingConfig`,
`useLLMConfig`, `useCollectionIndex` (new in 0.3.0 — live per-collection
ANN index lifecycle; polls `GET /v1/namespaces/{name}/index` at 3 s during
`building`/`ready`, stops for terminal states), plus their associated types —
intended for hosts building custom composite views (the same primitives the
views above are built on).

**Capabilities** (0.2.0): `StudioCapabilities` gained an optional
`clientEmbeddingFallback?: boolean` field, defaulting to unset/off. It gates
`DocumentUploadTab`'s (and therefore `ToolsWorkspace`'s Upload tab) extra
client-driven form fields — embedding provider/model/apiKey/endpoint, chunk
size/overlap, contextual enrichment — sent alongside the upload request for
a host whose own ingest route can fall back to its own chunk+embed+insert
pipeline when the node has no server embed provider configured. Off by
default, so every existing capabilities object (none of which set this
field before 0.2.0) is unaffected.

**Domain types**: shared value types only (`./types/valori`,
`NsEvent`/`NsAuditResponse`) — no host-specific types.

Not exported: anything under `src/lib/hooks/useProvisionerStatus.ts` — a
Cloud-provisioning-specific concept that doesn't generalize across hosts and
has no current consumer; the file remains in the source tree but isn't part
of the package contract.

## Versioning

Pre-1.0, so the public API is still expected to move — treat `0.x` as
"stable enough to build on, not yet frozen." Within that:

- **PATCH** (`0.1.x`) — bug fix, no public API change (exports, prop
  shapes, and runtime contract interfaces are unchanged).
- **MINOR** (`0.x.0`) — backward-compatible addition: a new export, a new
  optional prop, a new tab, a new hook. Existing consumers keep working
  without any code change.
- **MAJOR** (`x.0.0`, or any `0.x → 0.y` bump before 1.0 that breaks
  compatibility) — a breaking change to anything a host currently depends
  on: a removed/renamed export, a changed required-prop shape, a changed
  `Transport`/`CredentialStore`/`StudioCapabilities` contract, or a
  behavior change a host would need to react to. Before 1.0, treat any
  breaking change as reason enough to bump the minor version and call it
  out explicitly in `CHANGELOG.md` — semver's "anything goes pre-1.0" is
  not an excuse to skip documenting breakage.

See `CHANGELOG.md` for the release history.

## Architectural guardrails

See [`docs/architecture/shared-studio.md`](../../docs/architecture/shared-studio.md)
in the `Valori-Kernel` repository for the rules governing when something
belongs in Studio vs. a host application.
