# RG8 — Claim / Contradiction Verification

RG8 provides an auditable structural verifier with `Supports`, `Contradicts`, `Neutral`, and `Unknown` outcomes. It compares subject, predicate, object/value, negation, and time scope; citation and vector similarity are never treated as semantic evidence. Verification receipts contain stable input IDs, verifier version/config, evidence references, confidence source, and a deterministic receipt hash.

Conflicting assertions coexist and verification is represented separately from the original assertions. The existing replicated metadata event path preserves receipts across WAL replay and snapshots.
