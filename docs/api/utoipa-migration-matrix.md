# Phase 2.5 — Utoipa / OpenAPI Migration Matrix & Generator Reproducibility

## 1. Overview of OpenAPI Contract Architecture

The public API contract for Valori Data Plane v1 is currently maintained via a **hybrid generated-subset model**:

```text
Rust DTO Annotations (crates/valori-node/src/api.rs)
        │
        ▼
   utoipa (ValoriApi)
        │
        ▼
Generated Subset (14 schemas, 0 paths)
        │
        ▼ (Conforms via tests/openapi_generated.rs)
Canonical Public OpenAPI Specification (api/openapi/valori-v1.yaml)
  (102 schemas, 79 paths, rich metadata)
        │
        ▼ (scripts/generate-api-types.sh)
TypeScript Wire Types (@valori/api-types / ui/api-types/src/valori-v1.ts)
```

---

## 2. Utoipa Migration Matrix by API Domain

| Domain | Schemas Total | Schemas Generated | Paths Total | Paths Generated | Utoipa Status | Phase 3 Migration Task |
|--------|---------------|-------------------|-------------|-----------------|---------------|------------------------|
| **Error Taxonomy** | 2 | 2 (`ApiError`, `ErrorCode`) | 0 | 0 | **Generated Subset** | Annotate standard error responses on all path items |
| **Collections** | 6 | 3 (`CreateCollectionRequest`, `CreateCollectionResponse`, `ListCollectionsResponse`) | 4 | 0 | **Partial** | Annotate GET/POST/DELETE `/v1/namespaces` paths |
| **Records** | 8 | 4 (`InsertRecordRequest`, `InsertRecordResponse`, `InsertReceiptJson`, `RequestId`) | 8 | 0 | **Partial** | Annotate POST/DELETE `/v1/records` & `/v1/vectors/batch-insert` |
| **Search** | 6 | 2 (`SearchHit`, `SearchResponse`) | 2 | 0 | **Partial** | Annotate POST `/v1/search` path |
| **Multi-Search** | 5 | 3 (`MultiSearchRequest`, `MultiSearchResponse`, `MultiSearchHit`) | 1 | 0 | **Partial** | Annotate POST `/v1/search/multi` path |
| **Indexes** | 8 | 0 | 5 | 0 | **Hand-Maintained** | Derive `IndexKind` and `MetricKind` schemas & paths |
| **Graph** | 12 | 0 | 10 | 0 | **Hand-Maintained** | Annotate `/v1/graph/*` nodes & edges DTOs & paths |
| **GraphRAG** | 8 | 0 | 4 | 0 | **Hand-Maintained** | Annotate `/v1/graphrag` DTOs & paths |
| **Memory** | 10 | 0 | 6 | 0 | **Hand-Maintained** | Annotate `/v1/memory/*` DTOs & paths |
| **Ingest** | 8 | 0 | 5 | 0 | **Hand-Maintained** | Annotate `/v1/ingest/*` chunking & embedding paths |
| **Snapshots** | 10 | 0 | 8 | 0 | **Hand-Maintained** | Annotate `/v1/snapshots/*` export/import paths |
| **Proof & Audit** | 9 | 0 | 6 | 0 | **Hand-Maintained** | Annotate `/v1/proof/*` & timeline paths |
| **Health & Meta** | 10 | 0 | 20 | 0 | **Hand-Maintained** | Reconcile `/health` shape & annotate meta paths |
| **TOTAL** | **102** | **14** | **79** | **0** | **Partial (14/102 schemas, 0/79 paths)** | Full path annotation owned by Phase 3 |

---

## 3. Metadata Preservation Strategy

`api/openapi/valori-v1.yaml` carries essential contract metadata that must NOT be lost during Utoipa migration:
- **`description`**: Detailed semantic contracts and Q16.16 numeric constraints.
- **`x-status`**: Machine-readable status tracker (`current` vs `target` vs `drift`).
- **`x-required-scope`**: Authorization requirements (`read_only` vs `read_write` vs `admin`).
- **`x-sdk`**: Method mapping rules for official SDK generators.

### Policy for Phase 3 Transition
Phase 3 will introduce Utoipa macro attribute extensions (`#[schema(example = ...)]`, custom doc comment attributes, or deterministic post-generation YAML enrichment) to preserve 100% of contract metadata before discarding hand-written YAML.

---

## 4. TypeScript Generator Pipeline (`@valori/api-types`)

- **Generator Command**: `./scripts/generate-api-types.sh`
- **Tooling**: `openapi-typescript@7`
- **Output Artifact**: `ui/api-types/src/valori-v1.ts`
- **Invariants**:
  1. `@valori/api-types` contains ONLY wire models (`components["schemas"]`).
  2. UI components (in `ui/src/` and `ui/studio/`) import wire types from `@valori/api-types` and maintain separate UI view-model interfaces.
  3. Generation is 100% reproducible and deterministic: running `./scripts/generate-api-types.sh` on clean input yields zero `git diff`.
