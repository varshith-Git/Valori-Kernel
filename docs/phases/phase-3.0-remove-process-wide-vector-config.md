# Phase — Remove Process-Wide Vector Config from Project Creation (UI)

## Goal

Complete the UI half of `phase-collection-scoped-vector-config.md`'s follow-up: strip
`dimension`, `index`, and `embedding` from the project-creation dialogs (local + Cloud),
and wire the collection-creation dialog (local + Cloud) to send `dimension` (required)
and `index` (optional) when creating a namespace — matching the now-existing
`POST /v1/namespaces { dimension?, metric?, index? }` contract.

## Delivered

### `ui/src/components/projects/CreateProjectDialog.tsx`

- Removed all `dim`, `index`, and `embed` inputs and state.
- `Props.onCreate` simplified from
  `(name, dim, index, replication, shardCount, embed)` to
  `(name, replication, shardCount)`.
- Added an informational callout:
  *"Vector dimensions are configured per-collection after creating the project."*
- `handleDuplicate` no longer copies `dim`/`index`/`embed` when cloning a project.

### `ui/src/components/collections/CreateCollectionDialog.tsx`

Full rewrite. New fields:

| Field | Type | Validation | Wire |
|---|---|---|---|
| Name | text | `[a-zA-Z0-9_-]`, ≤ 64 chars, no "default" collision | `name` |
| Dimension | number | required, integer 1–65535 | `dimension` |
| Index | select | brute / auto / hnsw / ivf / bq | `index` (omitted if `brute`) |

- `Props.onCreate` updated to `(name, dim, index?) => Promise<void>`.
- Reset now clears dim and index state on cancel/close.
- Namespace preview line removed (new collections are bare; the prefix era is over).

### `ui/src/lib/hooks/useCollections.ts`

`create(name, dim, index?)` — forwards `dimension: dim` and, when non-brute, `index`
to `POST /api/namespaces`, which is the Next.js proxy for `POST /v1/namespaces`.

### `ui/src/components/collections/CollectionList.tsx`

`Props.onCreate` type updated to match the new `create` signature:
`(name: string, dim: number, index?: "brute"|"hnsw"|"ivf"|"bq"|"auto") => Promise<void>`.

### `ui/src/app/projects/[name]/page.tsx` (`CollectionsTab`)

No change needed — `CollectionsTab` passes `useCollections.create` straight to
`CollectionList.onCreate`, and both signatures moved together.

### `ui/src/app/api/projects/route.ts` (POST handler)

- `dim` and `index` removed from the request body schema.
- Internal constants `dim = 0` (sentinel — "no process-wide default") and
  `index = "brute"` passed to `daemon.createProject` to keep the daemon manifest
  contract satisfied without surfacing these as user-visible fields.
- `embed` kept in the body schema (backward-compat at the API level; UI no longer sends it).

### `ui/src/app/page.tsx`

- `CreateProjectDialog.onCreate` updated at the call site in the home page:
  `async (name, replication, shardCount)`.
- `handleDuplicate` stops forwarding `dim`/`index`/`embed`.

### `ui/src/components/layout/Sidebar.tsx`

Same `CreateProjectDialog.onCreate` call-site update as `page.tsx`.

### `ui/src/app/cloud/projects/[id]/CollectionsPanel.tsx`

Added `dim` state + number input to the inline creation form.
`handleCreate` parses and validates the integer (1–65535), then passes it to
`create(name, dimNum)`.

## Findings

1. **The sentinel `dim = 0` in the API route is a deliberate short-term hack.**
   `daemon.createProject` requires a `dim` field in its JSON payload — the daemon
   manifest still has this field for legacy projects that set it at process startup
   via `VALORI_DIM`. New-style projects will *always* configure dim at the collection
   level (`POST /v1/namespaces { dimension }`) before inserting any vector, so `0` is
   never actually used as a real dimension. The field should be made `Option<u32>` in
   the daemon API in a follow-up once all daemon consumers are confirmed off the old
   path.

2. **`ManifestProject.dim` and `.index` fields are still present in the TypeScript
   interface** (useProjectManifest.ts). The daemon still returns them in its project
   listing response for backward compat with old projects. They should be deprecated
   once the daemon-side legacy path is removed.

3. **Cloud `CollectionsPanel` uses an inline form instead of the shared dialog.**
   It now has a dimension input, but no index picker — matching minimum viable parity
   with the local path. Adding index selection is a follow-up; for now cloud
   collections always default to `brute` at the node level.

4. **TypeScript build passes clean** (`npx tsc --noEmit` exit 0, no errors or warnings).

## Validation

```
npx tsc --noEmit    (ui)    exit 0 — zero type errors or warnings
cargo test -p valori-kernel -p valori-node    exit 0 — all passed
```

This phase makes **zero changes to Rust source files**. Every existing kernel
and node test continues to pass without modification. No new Rust tests were
added — the correctness surface is pure TypeScript UI code, verified by `tsc`.

Manual smoke test:
```bash
# 1. Open UI, create a new project — confirm no dim/index field is present
# 2. Open project → Collections tab → "New collection"
#    Fill: name="docs", dimension=768, index=HNSW → Create
# 3. curl http://localhost:<port>/v1/namespaces
#    → [{name:"default",...},{name:"docs",id:1,dimension:768,index:"hnsw"}]
# 4. POST /v1/records with a 384-dim vector to docs
#    → 400 DimensionMismatch (pass — dimension enforced by kernel)
# 5. Duplicate a project from the home page
#    → new project has no dim/index/embed fields forwarded (pass)
# 6. Cloud CollectionsPanel: click "New collection", fill name + dimension
#    → collection created with explicit dimension (pass)
```

## Follow-ups

| Item | Phase |
|---|---|
| Make `daemon.createProject` accept `dim: Option<u32>` and stop requiring it | daemon cleanup |
| Deprecate `ManifestProject.dim` / `.index` in the TypeScript interface | UI cleanup |
| Cloud `CollectionsPanel`: add index picker alongside the dimension input | Cloud UI |
| Python SDK: `create_collection(name, dimension=, index=)` (started but not shipped in this phase) | Python SDK |
| Cluster-mode per-collection ANN index (currently brute-force-equivalent) | next Rust phase |
| Per-namespace dim in snapshot format (mixed-dimension restore) | valori-kernel |
