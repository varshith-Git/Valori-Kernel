# RG7 — Canonical Entity Resolution

RG7 separates source mentions from canonical entities. Resolution is deterministic and additive: Unicode/case/whitespace/punctuation normalization, compatible entity types, explicit aliases, and source/document/chunk/hash context form the identity key. No fuzzy or LLM merge is authoritative.

Mention and entity records are serializable and can be stored in the existing replicated `SetMeta` sidecar, so standalone, Raft replay, and snapshots preserve the same IDs and mappings. Existing record IDs, RG5 provenance, RG6 assertion IDs, and GraphRAG traversal remain unchanged.
