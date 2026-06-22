# Phase 5.1 — Verifiable AI: Proof-Carrying Answers + Compliance Pack

## Goal

Turn Valori's deterministic, BLAKE3-chained core into a sellable "Verifiable AI"
story for regulated enterprise buyers: every AI answer should carry cryptographic
proof of what it was based on (A1), and a single button should produce a
regulator-ready evidence bundle for a collection (B1).

## Delivered

UI-only vertical slice (no Rust kernel changes — reuses existing endpoints).

**A1 — Proof-Carrying Answers**

- `ui/src/app/api/why/route.ts` — the Ask backend now captures a receipt
  atomically with each answer: SHA-256 content hash of every cited chunk
  (and graph-expanded chunk), plus the global BLAKE3 state hash fetched from
  `/v1/proof/state` at answer time. Added `sha256()` and `fetchGlobalStateHash()`.
- `ui/src/lib/receipts.ts` — NEW shared receipt format + helpers
  (`finalizeReceipt`, `verifyReceiptFingerprint`, `sha256hex`). The receipt is
  self-verifying: `receipt_sha256` is SHA-256 of the canonical JSON with that
  field nulled.
- `ui/src/components/collections/AskTab.tsx` — finalizes the server receipt
  (adds question, answer hash, model identity, fingerprint), persists it in the
  per-namespace Ask history, and renders a `ProofReceipt` panel under each
  answer with download-JSON, print-to-PDF, copy-fingerprint, and raw-JSON views.

**B1 — Compliance Pack Exporter**

- `ui/src/components/collections/CompliancePackTab.tsx` — NEW. Assembles a signed
  bundle for the namespace: integrity attestation (namespace + global hashes,
  counts), tamper status vs. the Certify baseline, all GDPR erasure certificates,
  and all answer-provenance receipts — mapped to EU AI Act / GDPR / SOC 2
  controls. SHA-256 self-fingerprint, JSON download, and a 5-section printable PDF.
- `ui/src/components/collections/GdprTab.tsx` — erasure certificates are now
  persisted to `valori:erasures:<namespace>` so the pack can bundle them.
- `ui/src/app/projects/[name]/[collection]/page.tsx` — new **Compliance** tab +
  guide-card entry + tooltip.
- `ui/src/app/help/page.tsx` — Feature Guide updated (cheat sheet + "Prove
  integrity" goal entries for both flagships).

**Storage keys used:** `valori:ask-history:<ns>` (receipts), `valori:tamper:<ns>`
(baseline, from Certify), `valori:erasures:<ns>` (erasure certs, new).

## Findings

- `ShredKey` remains `NotImplemented` in the kernel apply path, so true
  crypto-erasure cannot yet be attested; the pack documents *physical* erasure
  (`DeleteRecord`) only. Tracked for B2.
- No node-side endpoint returns a record's text by ID in one call cheaply; the
  receipt hashes the chunk text already present in the search-result metadata,
  which is sufficient for the binding but means chunks without text metadata
  appear as `content_sha256: null`.
- Receipt/erasure/baseline evidence currently lives in browser `localStorage`,
  so a Compliance Pack reflects activity from *this browser*. Server-side
  receipt persistence is a follow-up (would also enable cross-device packs).

## Validation

- `npx tsc --noEmit` — clean, 0 errors.
- Manual smoke (requires a running node + configured embed/LLM):
  1. Ask a question → expand "🔏 Proof-carrying receipt" → confirm chunk hashes,
     global state hash, and fingerprint render; download + print work.
  2. GDPR tab → erase a record → confirm `valori:erasures:<ns>` is written.
  3. Compliance tab → Generate pack → confirm all 5 sections populate, tamper
     status reflects the Certify baseline, and JSON/PDF export work.
- Rust crates untouched; `cargo test` not re-run (no kernel/node/consensus
  changes in this phase).

## Follow-ups

- **A2 — Time-Travel RAG** ("as-of" retrieval): needs a node HTTP endpoint that
  replays to event N before searching (CLI `replay_query` already does this).
- **B2 — Crypto multi-tenant + provable offboarding**: requires implementing
  `ShredKey` in the kernel apply path + a key vault.
- **B4 — Lineage / chain-of-custody map**: visual provenance per answer.
- Server-side receipt store so Compliance Packs are browser-independent.
