#!/usr/bin/env python3
"""
Phase 3.10 — SOC 2 Type II evidence collection automation.

Collects and exports evidence for the following control families:
  CC6  — Logical access (API keys, bearer tokens)
  CC7  — System operations (audit log integrity, state hash convergence)
  CC9  — Risk mitigation (crypto-shredding, backup verification)
  A1   — Availability (health checks, replication status)

Usage:
  python3 scripts/soc2/collect_evidence.py \
    --node http://localhost:3000 \
    --out  evidence/$(date +%Y-%m-%d)

Output directory contains:
  audit_log_integrity.json   — BLAKE3 proof + event count
  replication_status.json    — cluster health + state hash per node
  api_key_roster.json        — masked key list (no raw tokens)
  health_snapshot.json       — /health response
  cluster_status.json        — /v1/cluster/status (if cluster mode)
  EVIDENCE_INDEX.md          — human-readable index with control mappings
"""

import os
import sys
import json
import argparse
import datetime
import urllib.request
import urllib.error
from pathlib import Path


def _get(url: str, token: str | None = None) -> dict | None:
    headers = {"Accept": "application/json"}
    if token:
        headers["Authorization"] = f"Bearer {token}"
    try:
        req = urllib.request.Request(url, headers=headers)
        with urllib.request.urlopen(req, timeout=10) as resp:
            return json.loads(resp.read())
    except urllib.error.HTTPError as e:
        if e.code in (401, 403, 404):
            return None
        raise
    except Exception:
        return None


def _save(out_dir: Path, filename: str, data: dict | None, label: str):
    if data is None:
        print(f"  [skip] {label} — endpoint unavailable or not configured")
        return
    path = out_dir / filename
    path.write_text(json.dumps(data, indent=2))
    print(f"  [ok]   {label} → {path}")


def collect(node_url: str, token: str | None, out_dir: Path):
    out_dir.mkdir(parents=True, exist_ok=True)
    ts = datetime.datetime.utcnow().isoformat() + "Z"
    print(f"\nCollecting SOC 2 evidence from {node_url}")
    print(f"Timestamp: {ts}\n")

    # CC7.2 — Audit log integrity (event log BLAKE3 proof)
    proof_state = _get(f"{node_url}/v1/proof/state", token)
    proof_event = _get(f"{node_url}/v1/proof/event-log", token)
    audit_evidence = {
        "collected_at": ts,
        "state_proof": proof_state,
        "event_log_proof": proof_event,
        "control": "CC7.2 — Audit log integrity via BLAKE3 chain",
    }
    _save(out_dir, "audit_log_integrity.json", audit_evidence, "CC7.2 Audit log integrity")

    # A1.2 — Health and availability
    health = _get(f"{node_url}/health", token)
    if health:
        health["collected_at"] = ts
    _save(out_dir, "health_snapshot.json", health, "A1.2 Health snapshot")

    # CC6.6 — API key roster (tokens masked — no raw values stored)
    keys = _get(f"{node_url}/v1/keys", token)
    if keys:
        keys["collected_at"] = ts
        keys["_note"] = "Raw tokens are never stored. This roster shows masked IDs and scopes only."
    _save(out_dir, "api_key_roster.json", keys, "CC6.6 API key roster (masked)")

    # A1.1 — Cluster health and replication
    cluster_status = _get(f"{node_url}/v1/cluster/status", token)
    if cluster_status:
        cluster_status["collected_at"] = ts
    _save(out_dir, "cluster_status.json", cluster_status, "A1.1 Cluster status")

    cluster_health = _get(f"{node_url}/v1/cluster/health", token)
    if cluster_health:
        cluster_health["collected_at"] = ts
    _save(out_dir, "cluster_health.json", cluster_health, "A1.1 Cluster health")

    # CC9 — Backup / snapshot evidence
    remote_snaps = _get(f"{node_url}/v1/storage/snapshots", token)
    if remote_snaps:
        remote_snaps["collected_at"] = ts
        remote_snaps["_control"] = "CC9 — Backup availability via object-store snapshot offload"
    _save(out_dir, "remote_snapshots.json", remote_snaps, "CC9 Remote snapshot inventory")

    # Metrics (Prometheus) — optional, not written to evidence bundle
    # (pull into your SIEM/Grafana stack separately)

    # ── Evidence index ────────────────────────────────────────────────────────
    index = f"""# SOC 2 Evidence Index

Collected: {ts}
Node: {node_url}

## Control mappings

| File | SOC 2 Control | Description |
|---|---|---|
| `audit_log_integrity.json` | CC7.2 | BLAKE3 chain proof — log integrity verifiable without trusting the node |
| `health_snapshot.json` | A1.2 | System availability — health endpoint response at collection time |
| `api_key_roster.json` | CC6.6 | Logical access — API key roster with scopes; no raw tokens stored |
| `cluster_status.json` | A1.1 | Cluster health — leader election, log indices, membership |
| `cluster_health.json` | A1.1 | Binary cluster health check |
| `remote_snapshots.json` | CC9 | Backup evidence — object-store snapshot inventory |

## Verification

To independently verify the BLAKE3 audit chain:

```bash
# Install valori-verify
cargo build -p valori-verify --release

# Replay and verify the event log
./target/release/valori-verify --log VALORI_EVENT_LOG_PATH
```

Compare the `final_state_hash` in `audit_log_integrity.json` against the
verifier output to confirm no tampering.

## Notes

- Evidence is collected automatically via `scripts/soc2/collect_evidence.py`.
- Schedule weekly collection via cron or CI for continuous SOC 2 evidence trail.
- Raw API tokens are **never** included. The `api_key_roster.json` lists only
  masked IDs, scopes, and creation timestamps.
"""
    index_path = out_dir / "EVIDENCE_INDEX.md"
    index_path.write_text(index)
    print(f"  [ok]   Evidence index → {index_path}")

    print(f"\nEvidence bundle written to: {out_dir}/")


def main():
    parser = argparse.ArgumentParser(description="Collect SOC 2 evidence from a Valori node")
    parser.add_argument("--node", default="http://localhost:3000", help="Node URL")
    parser.add_argument("--token", default=os.environ.get("VALORI_AUTH_TOKEN"), help="Bearer token")
    parser.add_argument("--out", default=f"evidence/{datetime.date.today()}", help="Output directory")
    args = parser.parse_args()

    collect(args.node, args.token, Path(args.out))


if __name__ == "__main__":
    main()
