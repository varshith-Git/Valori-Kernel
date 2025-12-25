# Multi-Architecture Determinism Validation

This GitHub Actions workflow provides **automated proof** that Valori's kernel produces bit-identical state across different CPU architectures.

## What This Proves

Every commit automatically runs identical test suites on:
- **x86_64** (Intel/AMD - Ubuntu)
- **ARM64** (Apple Silicon - macOS)  
- **WASM32** (WebAssembly)

The workflow compares cryptographic state hashes. If ANY divergence is detected, the build fails.

## Badge

[![Determinism: Verified](https://img.shields.io/badge/determinism-verified-brightgreen)](https://github.com/YOUR_ORG/Valori-Kernel/actions/workflows/multi-arch-determinism.yml)

## Test Strategy

Each architecture runs:
1. Insert 100 records with identical seed data
2. Compute `kernel_state_hash`
3. Save hash to artifact
4. Final step compares all hashes - must be identical

## Results

See [latest workflow run](https://github.com/YOUR_ORG/Valori-Kernel/actions/workflows/multi-arch-determinism.yml) for proof.

**Example output**:
```
x86_64 hash: [103, 22, 141, 66, 192, 16, 92, 106, ...]
ARM64  hash: [103, 22, 141, 66, 192, 16, 92, 106, ...]
WASM32 hash: [103, 22, 141, 66, 192, 16, 92, 106, ...]

✅ ALL HASHES MATCH - Determinism verified!
```

This is your **proof of determinism** for embedded partners.
