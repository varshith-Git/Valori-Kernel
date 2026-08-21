# Valori Data Plane `/health` Endpoint Migration Analysis

> [!IMPORTANT]
> **Strict Phase 2.5 Policy**: This document is **analysis only**. NO code changes to `/health` handlers or wire types are implemented in Phase 2.5, preserving full compatibility for existing consumers (UI, CLI, Docker, MCP).

---

## 1. Current Behavior & Divergence

Standalone mode and Raft cluster mode currently emit different JSON response shapes for `GET /health`:

### Standalone Health (`EngineHealth`)
```json
{
  "status": "ok",
  "version": "1.0.0",
  "collections": 3,
  "persistence": "event_log",
  "records": { "live": 120, "slots_used": 120, "capacity": 256, "fill_pct": 46.875 },
  "nodes": { "live": 10, "slots_used": 10, "capacity": 64, "fill_pct": 15.625 },
  "edges": { "live": 15, "slots_used": 15, "capacity": 64, "fill_pct": 23.4375 },
  "event_log_height": 145,
  "embed_enabled": false,
  "shard_count": 1
}
```

### Cluster Health (`ClusterNodeHealth`)
```json
{
  "status": "ok",
  "node_id": 1,
  "role": "leader",
  "leader_id": 1,
  "term": 4,
  "raft_state": "Leader",
  "state_hash": "a1b2c3d4...",
  "members": 3,
  "shard_count": 1
}
```

---

## 2. Affected Consumers Inventory

| Consumer | File Location | Relies On | Status |
|----------|---------------|-----------|--------|
| **UI Dashboard** | `ui/src/lib/hooks/useHealth.ts` | `status`, `version`, `records`, `nodes`, `edges` | Uses optional fields safely |
| **Python SDK / CLI** | `python/valoricore/remote.py` | `status` check | Checks HTTP 200 & `status == "ok"` |
| **Docker Compose** | `docker-compose.yml` | HTTP 200 status code | `curl -f http://localhost:3000/health` |
| **MCP Server** | `crates/valori-mcp/src/main.rs` | `status` check | Checks `status == "ok"` |

---

## 3. Canonical Migration Proposal (Phase 3+)

To achieve unified OpenAPI representation without breaking existing clients:

### Proposed Unified `HealthResponse` Envelope
```json
{
  "status": "ok",
  "mode": "standalone",
  "version": "1.0.0",
  "engine": {
    "records": { "live": 120, "slots_used": 120, "capacity": 256, "fill_pct": 46.875 },
    "nodes": { "live": 10, "slots_used": 10, "capacity": 64, "fill_pct": 15.625 },
    "edges": { "live": 15, "slots_used": 15, "capacity": 64, "fill_pct": 23.4375 }
  },
  "cluster": null
}
```
In Cluster mode, `mode` is set to `"cluster"` and `cluster` carries Raft leader/role metadata.

---

## 4. Rollout Strategy
1. Maintain top-level `status` and `version` fields for backward compatibility.
2. Nest engine pool stats under `engine` object while leaving legacy top-level keys deprecated.
3. Migrate `useHealth.ts`, CLI, and MCP server to read from the unified envelope.
