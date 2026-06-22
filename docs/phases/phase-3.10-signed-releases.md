# Phase 3.10 — Signed releases + SBOM

## Goal

Every release binary and Docker image is cryptographically signed using cosign keyless signing (Sigstore), an SPDX 2.3 SBOM is generated and attached, and SOC 2 Type II evidence collection is automated. No long-lived signing keys are required.

## Delivered

### Release workflow (`.github/workflows/release.yml`)

Triggered on `v*` tags. Four jobs:

1. **`build`** — Cross-compiles `valori-node` for 4 targets:
   - `linux/amd64` (ubuntu-22.04)
   - `linux/arm64` (via `cross`)
   - `darwin/amd64` (macos-14)
   - `darwin/arm64` (macos-14)

2. **`sbom`** — Generates `valori-sbom.spdx.json` via `cargo-sbom` (SPDX 2.3 format listing all Rust crate dependencies with license identifiers).

3. **`sign-and-release`** — Signs all binaries + SBOM with cosign keyless signing (GitHub Actions OIDC → Sigstore transparency log). Creates a GitHub Release with:
   - 4 binaries + `.pem` + `.sig` per binary (12 files)
   - `valori-sbom.spdx.json` + `.pem` + `.sig`
   - `SHA256SUMS.txt`

4. **`sign-docker`** — Builds multi-arch Docker image (`linux/amd64` + `linux/arm64`), pushes to GHCR, signs the image digest with cosign.

### SOC 2 workflows

| File | Purpose |
|---|---|
| `.github/workflows/soc2-evidence.yml` | Weekly evidence collection (every Sunday 02:00 UTC). Uploads 90-day-retained artifact bundle. |
| `scripts/soc2/collect_evidence.py` | Standalone evidence collector: hits `/v1/proof/state`, `/v1/proof/event-log`, `/v1/keys`, `/v1/cluster/status`, `/v1/storage/snapshots`. Outputs JSON files + `EVIDENCE_INDEX.md` with control mappings. |

**Control coverage:**

| File | SOC 2 Control |
|---|---|
| `audit_log_integrity.json` | CC7.2 — Audit log integrity via BLAKE3 chain |
| `health_snapshot.json` | A1.2 — System availability |
| `api_key_roster.json` | CC6.6 — Logical access (tokens masked) |
| `cluster_status.json` | A1.1 — Cluster health |
| `remote_snapshots.json` | CC9 — Backup availability |

### Verification

Any release binary can be independently verified without trusting GitHub:

```bash
cosign verify-blob \
  --certificate valori-node-linux-amd64.pem \
  --signature   valori-node-linux-amd64.sig \
  --certificate-identity-regexp "https://github.com/varshith-git/valori-kernel" \
  --certificate-oidc-issuer "https://token.actions.githubusercontent.com" \
  valori-node-linux-amd64
```

The certificate chain is anchored at Sigstore's public Rekor transparency log — no private key management required.

### cargo-deny (pre-existing + Phase 3.10 scope)

The `cargo-deny` workflow (Phase 1.10) already runs `check` covering:
- `advisories` — blocks on vulnerabilities and unsound crates (daily schedule)
- `licenses` — blocks on forbidden licenses
- `bans` — blocks on prohibited crates (openssl)
- `sources` — blocks on non-crates.io registries

Phase 3.10 confirms this covers the vulnerability scan requirement from the roadmap. No new `deny.toml` changes needed.

## Findings

- Cosign keyless signing requires `id-token: write` permission on the job — not on the workflow level, since the sign job runs in `sign-and-release`, not in `build`.
- `cargo-sbom` outputs SPDX 2.3 JSON by default when passed `--output-format spdx_json_2_3`. It recursively includes all workspace crates.
- `softprops/action-gh-release@v2` is used instead of the deprecated `actions/create-release` — it handles both release creation and asset upload in one step.
- The Docker signing uses the image digest (not tag) per cosign best practice — tags are mutable, digests are not.

## Validation

Workflow syntax validation:
```bash
# GitHub CLI validates workflow files on push; no local tool required.
# For local check: install actionlint
actionlint .github/workflows/release.yml
actionlint .github/workflows/soc2-evidence.yml
```

Evidence collection script syntax:
```bash
python3 -c "import ast; ast.parse(open('scripts/soc2/collect_evidence.py').read()); print('ok')"
```

Live end-to-end test: push a `v0.3.0-rc.1` tag and verify the GitHub Release is created with all 20 assets.

## Follow-ups

- Phase 4 — After the Kubernetes operator is ready, add cosign attestation on the Helm chart release.
- SOC 2 Type II audit: evidence trail starts on 2026-06-22 with weekly collection. A full quarter of evidence (90 days) will be available by 2026-09-22.
