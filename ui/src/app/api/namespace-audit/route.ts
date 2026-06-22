import { NextRequest, NextResponse } from "next/server";
import crypto from "crypto";

import { getApiUrl } from "@/lib/server/connection";
const TOKEN = process.env.VALORI_AUTH_TOKEN;

function h(): Record<string, string> {
  const headers: Record<string, string> = { "Content-Type": "application/json" };
  if (TOKEN) headers["Authorization"] = `Bearer ${TOKEN}`;
  return headers;
}

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

// Extract event kind from raw string like "Event ID 0: InsertRecord (Record 1, ...)"
function parseKind(raw: string): string {
  const m = raw.match(/:\s+([A-Za-z]+(?:[A-Z][a-z]+)*)/);
  return m?.[1] ?? "Unknown";
}

// Extract all record IDs from a raw event string
function parseRecordIds(raw: string): number[] {
  const ids: number[] = [];
  for (const m of raw.matchAll(/\bRecord\s+(\d+)/g)) ids.push(parseInt(m[1], 10));
  return ids;
}

// Extract all node IDs from a raw event string
function parseNodeIds(raw: string): number[] {
  const ids: number[] = [];
  for (const m of raw.matchAll(/\bNode\s+(\d+)/g)) ids.push(parseInt(m[1], 10));
  for (const m of raw.matchAll(/NodeId\((\d+)\)/g)) ids.push(parseInt(m[1], 10));
  return [...new Set(ids)];
}

// GET /api/namespace-audit?namespace=...
export async function GET(req: NextRequest) {
  const namespace = req.nextUrl.searchParams.get("namespace") ?? "default";

  // Parallel fetch: nodes in namespace + full timeline + global proof
  const [nodesRes, timelineRes, proofRes] = await Promise.allSettled([
    fetch(`${getApiUrl()}/graph/nodes?collection=${encodeURIComponent(namespace)}`, { headers: h() }),
    fetch(`${getApiUrl()}/timeline`, { headers: h() }),
    fetch(`${getApiUrl()}/v1/proof/event-log`, { headers: h() }),
  ]);

  // -- Parse nodes --------------------------------------------------------------
  type GraphNode = { node_id: number; record_id: number | null };
  const nodes: GraphNode[] =
    nodesRes.status === "fulfilled" && nodesRes.value.ok
      ? ((await nodesRes.value.json().catch(() => ({ nodes: [] }))) as { nodes?: GraphNode[] }).nodes ?? []
      : [];

  const nsRecordIds = new Set<number>();
  const nsNodeIds   = new Set<number>();
  for (const n of nodes) {
    nsNodeIds.add(n.node_id);
    if (n.record_id !== null) nsRecordIds.add(n.record_id);
  }

  // -- Parse timeline -----------------------------------------------------------
  const rawEvents: string[] =
    timelineRes.status === "fulfilled" && timelineRes.value.ok
      ? await timelineRes.value.json().catch(() => [])
      : [];

  const totalEvents = rawEvents.length;

  // Filter to events that touch this namespace's records or nodes
  const nsEvents: NsEvent[] = [];
  for (let i = 0; i < rawEvents.length; i++) {
    const raw = rawEvents[i];
    const recordIds = parseRecordIds(raw);
    const nodeIds   = parseNodeIds(raw);
    const touches =
      recordIds.some((id) => nsRecordIds.has(id)) ||
      nodeIds.some((id) => nsNodeIds.has(id));
    if (touches) {
      nsEvents.push({
        event_id: i,
        raw,
        kind: parseKind(raw),
        record_ids: recordIds,
        node_ids: nodeIds,
      });
    }
  }

  // -- Namespace proof — SHA-256 of sorted event IDs ----------------------------
  const nsEventIds = nsEvents.map((e) => e.event_id).sort((a, b) => a - b);
  const nsProofHash = crypto
    .createHash("sha256")
    .update(nsEventIds.join(","))
    .digest("hex");

  // -- Global proof -------------------------------------------------------------
  type ProofResp = { final_state_hash?: string; event_log_hash?: string; event_count?: number };
  const proof: ProofResp =
    proofRes.status === "fulfilled" && proofRes.value.ok
      ? await proofRes.value.json().catch(() => ({}))
      : {};

  const response: NsAuditResponse = {
    namespace,
    record_count: nsRecordIds.size,
    node_count:   nsNodeIds.size,
    ns_record_ids: [...nsRecordIds].sort((a, b) => a - b),
    ns_node_ids:   [...nsNodeIds].sort((a, b) => a - b),
    events: nsEvents,
    total_events: totalEvents,
    ns_event_ids: nsEventIds,
    ns_proof_hash: nsProofHash,
    global_state_hash:    proof.final_state_hash ?? null,
    global_event_log_hash: proof.event_log_hash  ?? null,
    global_event_count:   proof.event_count       ?? null,
  };

  return NextResponse.json(response);
}
