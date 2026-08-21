// Mirrors the /namespace-audit node-proxy response shape (previously
// imported as a type-only import from the host's own Next.js route file —
// not resolvable outside that host's app, so inlined here instead; the
// route handler itself, and the parsing helpers beside it, stay host-side).
export interface NsEvent {
  event_id: number;
  raw: string;
  kind: string;
  record_ids: number[];
  node_ids: number[];
}

export interface NsAuditResponse {
  namespace: string;
  record_count: number;
  node_count: number;
  ns_record_ids: number[];
  ns_node_ids: number[];
  events: NsEvent[];
  total_events: number;
  ns_event_ids: number[];
  /** SHA-256 of sorted event IDs — reproducible namespace proof */
  ns_proof_hash: string;
  /** Global BLAKE3 state hash */
  global_state_hash: string | null;
  /** Global event log BLAKE3 hash */
  global_event_log_hash: string | null;
  global_event_count: number | null;
  error?: string;
}
