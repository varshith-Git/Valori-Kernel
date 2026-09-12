# RG6 — Evidence-Backed Assertions

RG6 records extracted relationships as auditable semantic assertions. Each assertion preserves subject, predicate, object, source evidence, extraction provenance, confidence, and a content-addressed assertion ID. Evidence fields are optional for compatibility with existing extractors; missing predicates fall back to the relationship description.

Assertions are stored in the existing audited metadata sidecar for both standalone and Raft/cluster ingestion. This keeps WAL, snapshot, and replay behavior unchanged while allowing conflicting assertions to coexist as separate edges and IDs. Citation, support, contradiction, and similarity remain distinct caller-level semantics.

The assertion ID is deterministic for identical source text hash, tuple, and evidence, and excludes allocated node/edge IDs.
