// Proof-Carrying Answer receipts (feature A1).
//
// Schema frozen at version "1.0". To make a breaking change: bump RECEIPT_VERSION,
// add a migration in verifyReceiptFingerprint, update the eval harness qa_sets.
//
// A receipt is a tamper-evident record binding an LLM answer to the exact,
// unaltered chunks that produced it and to the node's global state hash at
// answer time. It is self-verifying: the receipt_sha256 field is SHA-256 of the
// canonical JSON with receipt_sha256 set to null.

export interface ReceiptChunkRef {
  record_id: number;
  chunk_index: number | null;
  source: string | null;
  score: number | null;
  rerank_score: number | null;  // null when no Tier-2 reranker was used
  enriched: boolean;            // true when a context sentence was committed with this chunk
  content_sha256: string | null;   // sha256:<hex> of the chunk text
  content_length: number;
}

export interface ReceiptGraphChunkRef {
  record_id: number;
  chunk_index: number;
  content_sha256: string | null;
}

// Graph nodes and edges traversed during answer synthesis (C2 provenance).
export interface ReceiptGraphNode {
  id: number;
  kind: number;       // NodeKind as u8: 1=Concept, 5=Document, 6=Chunk, etc.
  label: string | null; // entity label if Concept, filename if Document, else null
}

export interface ReceiptGraphEdge {
  id: number;
  from: number;
  to: number;
  kind: number;       // EdgeKind as u8: 4=Mentions, 5=RefersTo, 6=ParentOf, etc.
}

// What the /api/why server returns, captured atomically with the answer.
export interface ServerReceiptPart {
  global_state_hash: string | null;
  captured_at: string;
  chunks: ReceiptChunkRef[];
  graph_chunks: ReceiptGraphChunkRef[];
  provenance_nodes?: ReceiptGraphNode[];  // C2: traversed graph nodes
  provenance_edges?: ReceiptGraphEdge[];  // C2: traversed graph edges
}

export interface AnswerReceipt {
  type: "ValoriProofCarryingAnswer";
  version: string;
  collection: string;
  question: string;
  answer_sha256: string | null;      // sha256 of the exact answer text
  answer_present: boolean;
  k: number;
  models: { embed: string; llm: string | null };
  state: {
    global_state_hash: string | null; // BLAKE3, atomic at answer time
    captured_at: string;
  };
  chunks: ReceiptChunkRef[];
  graph_chunks: ReceiptGraphChunkRef[];
  provenance_nodes: ReceiptGraphNode[];   // C2: graph nodes traversed for this answer
  provenance_edges: ReceiptGraphEdge[];   // C2: graph edges traversed for this answer
  verification: string;
  receipt_sha256: string | null;     // self-fingerprint (null while hashing)
}

const RECEIPT_VERSION = "1.0";

const VERIFY_INSTRUCTIONS =
  "To verify: (1) replay events.log through valori-verify and confirm the final " +
  "state hash equals state.global_state_hash; (2) fetch each record by id, SHA-256 " +
  "its text metadata, and confirm it matches content_sha256; (3) recompute " +
  "receipt_sha256 as SHA-256 of this document with receipt_sha256 set to null. " +
  "If all three hold, the answer was grounded in exactly these unaltered chunks " +
  "at the recorded state.";

// SHA-256 of a UTF-8 string via Web Crypto, prefixed for self-describing hashes.
export async function sha256hex(text: string): Promise<string> {
  const buf = await crypto.subtle.digest("SHA-256", new TextEncoder().encode(text));
  return (
    "sha256:" +
    Array.from(new Uint8Array(buf))
      .map((b) => b.toString(16).padStart(2, "0"))
      .join("")
  );
}

// Combine the server receipt part with client-known context (question, answer,
// models) and compute the self-fingerprint.
export async function finalizeReceipt(input: {
  server: ServerReceiptPart;
  collection: string;
  question: string;
  answer: string | null;
  k: number;
  embedModel: string;
  llmModel: string | null;
}): Promise<AnswerReceipt> {
  const partial: AnswerReceipt = {
    type: "ValoriProofCarryingAnswer",
    version: RECEIPT_VERSION,
    collection: input.collection,
    question: input.question,
    answer_sha256: input.answer ? await sha256hex(input.answer) : null,
    answer_present: !!input.answer,
    k: input.k,
    models: { embed: input.embedModel, llm: input.llmModel },
    state: {
      global_state_hash: input.server.global_state_hash,
      captured_at: input.server.captured_at,
    },
    chunks: input.server.chunks,
    graph_chunks: input.server.graph_chunks,
    provenance_nodes: input.server.provenance_nodes ?? [],
    provenance_edges: input.server.provenance_edges ?? [],
    verification: VERIFY_INSTRUCTIONS,
    receipt_sha256: null,
  };

  const fingerprint = await sha256hex(JSON.stringify(partial));
  return { ...partial, receipt_sha256: fingerprint };
}

// Re-verify a receipt's self-fingerprint (does not re-fetch chunks — that
// requires the node + event log). Returns true if the stored fingerprint
// matches a recomputation.
export async function verifyReceiptFingerprint(receipt: AnswerReceipt): Promise<boolean> {
  if (!receipt.receipt_sha256) return false;
  const { receipt_sha256, ...rest } = receipt;
  const recomputed = await sha256hex(JSON.stringify({ ...rest, receipt_sha256: null }));
  return recomputed === receipt_sha256;
}
