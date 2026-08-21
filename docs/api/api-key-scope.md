# Valori API Key Collection-Scoped Authorization Security Analysis

> [!IMPORTANT]
> **Strict Phase 2.5 Policy**: This document is **analysis only**. NO authorization enforcement changes for `ApiKeyRecord.collection` are implemented in Phase 2.5. Current key scope enforcement (`read_only`, `read_write`, `admin`) remains unchanged.

---

## 1. Current State of Collection Scoping

In `crates/valori-metadata`, API keys are represented by `ApiKeyRecord`:
```rust
pub struct ApiKeyRecord {
    pub key_id: String,
    pub name: String,
    pub scope: ApiKeyScope, // ReadOnly, ReadWrite, Admin
    pub collection: Option<String>,
    pub created_at: u64,
}
```

### Gap Analysis
- `ApiKeyRecord.collection` is exposed in API key management endpoints (`POST /v1/cluster/api-keys`).
- However, `valori-node` authorization guards (`required_scope`) only check `ApiKeyScope` levels (`read_only` vs `read_write` vs `admin`).
- Requests to Collection-scoped endpoints (e.g. `POST /v1/records`, `POST /v1/search`) validate global scope permissions but do NOT verify if `ApiKeyRecord.collection` matches the target Collection in the request payload.

---

## 2. Risk Assessment

If a client generates an API key with `collection: "tenant-a"` and `scope: ReadWrite`:
- **Current Behavior**: The client can mutate records in `tenant-b` if it provides `collection: "tenant-b"` in the request body because the node checks `scope == ReadWrite` globally.
- **Security Implication**: Claiming Collection-level isolation based on `ApiKeyRecord.collection` is currently misleading until enforcement is implemented in server auth middleware.

---

## 3. Recommended Security Architecture (Dedicated Security Phase)

1. **Authorization Middleware Extension**:
   - Extract `collection` from request URL (`/v1/namespaces/{name}`) or JSON body (`{ "collection": "name" }`).
   - If `ApiKeyRecord.collection` is `Some(allowed_col)`, assert `allowed_col == target_collection`.
   - If mismatch, return `403 FORBIDDEN` with code `forbidden` and message `"API key is restricted to collection '{allowed_col}'"`.
2. **Wildcard & Multi-Collection Policy**:
   - `collection: None` implies unrestricted multi-collection access (subject to global scope).
   - Cross-collection endpoints (`POST /v1/search/multi`, `/v1/graphrag`) must reject single-collection keys unless all target collections match `allowed_col`.
