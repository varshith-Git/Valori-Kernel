import { NextRequest, NextResponse } from "next/server";

import { getApiUrl } from "@/lib/server/connection";
const TOKEN = process.env.VALORI_AUTH_TOKEN;

function h(): Record<string, string> {
  const headers: Record<string, string> = { "Content-Type": "application/json" };
  if (TOKEN) headers["Authorization"] = `Bearer ${TOKEN}`;
  return headers;
}

// Check if a graph node actually exists before trying to delete it.
// Avoids sending a DeleteNode event for an already-deleted node, which
// causes the kernel to return InvalidOperation and roll back the WAL buffer.
async function nodeExists(nodeId: number): Promise<boolean> {
  try {
    const res = await fetch(`${getApiUrl()}/graph/node/${nodeId}`, { headers: h() });
    return res.ok;
  } catch {
    return false;
  }
}

// Delete a single graph node sequentially (not in parallel).
// Returns true on success or if the node was already gone (idempotent).
async function deleteNode(nodeId: number): Promise<boolean> {
  if (!(await nodeExists(nodeId))) return true; // already gone — skip
  const res = await fetch(`${getApiUrl()}/graph/node/${nodeId}`, {
    method: "DELETE",
    headers: h(),
  });
  return res.ok;
}

// DELETE /api/documents/:nodeId?collection=...
// Cascades: chunk records → chunk nodes → document node
export async function DELETE(
  req: NextRequest,
  { params }: { params: Promise<{ nodeId: string }> }
) {
  const { nodeId } = await params;
  const docNodeId = parseInt(nodeId, 10);
  if (isNaN(docNodeId)) {
    return NextResponse.json({ error: "invalid nodeId" }, { status: 400 });
  }

  const collection = req.nextUrl.searchParams.get("collection") ?? "default";
  let deletedRecords = 0;
  let deletedNodes = 0;
  const errors: string[] = [];

  try {
    // 1. Get chunk edges for this document
    const edgesRes = await fetch(`${getApiUrl()}/graph/edges/${docNodeId}`, { headers: h() });
    const edgesData = edgesRes.ok
      ? await edgesRes.json().catch(() => ({ edges: [] })) as { edges?: { to_node: number }[] }
      : { edges: [] };

    // Deduplicate chunk node IDs — duplicate edges would cause double-delete → InvalidOperation
    const chunkNodeIds = [...new Set((edgesData.edges ?? []).map((e) => e.to_node))];

    // 2. Build chunk_node → record_id map from the namespace's node list
    const nodesRes = await fetch(
      `${getApiUrl()}/graph/nodes?collection=${encodeURIComponent(collection)}`,
      { headers: h() }
    );
    const nodesData = nodesRes.ok
      ? await nodesRes.json().catch(() => ({ nodes: [] })) as { nodes?: { node_id: number; record_id: number | null }[] }
      : { nodes: [] };
    const nodeToRecord = new Map<number, number>();
    for (const n of nodesData.nodes ?? []) {
      if (n.record_id !== null) nodeToRecord.set(n.node_id, n.record_id);
    }

    // 3. Delete chunk records (vectors) in parallel — record deletes are safe to parallelise
    const recordIds = [...new Set(
      chunkNodeIds
        .map((id) => nodeToRecord.get(id))
        .filter((id): id is number => id !== undefined)
    )];

    await Promise.all(
      recordIds.map(async (recordId) => {
        const res = await fetch(`${getApiUrl()}/v1/delete`, {
          method: "POST",
          headers: h(),
          body: JSON.stringify({ id: recordId }),
        });
        if (res.ok) deletedRecords++;
        else errors.push(`record ${recordId}: HTTP ${res.status}`);
      })
    );

    // 4. Delete chunk graph nodes SEQUENTIALLY — the kernel's event committer
    //    serialises WAL writes anyway; firing in parallel just races and causes
    //    one thread to see an already-deleted node → InvalidOperation → WAL rollback.
    for (const chunkNodeId of chunkNodeIds) {
      const ok = await deleteNode(chunkNodeId);
      if (ok) deletedNodes++;
      else errors.push(`chunk node ${chunkNodeId}: delete failed`);
    }

    // 5. Delete the document node itself
    const docOk = await deleteNode(docNodeId);
    if (docOk) deletedNodes++;
    else errors.push(`doc node ${docNodeId}: delete failed`);

    const ok = errors.length === 0;
    return NextResponse.json(
      {
        ok,
        deleted_records: deletedRecords,
        deleted_nodes: deletedNodes,
        errors: ok ? undefined : errors,
        error: ok ? undefined : `${errors.length} step(s) failed — some data may be orphaned`,
      },
      { status: ok ? 200 : 500 }
    );
  } catch (err) {
    return NextResponse.json(
      { error: err instanceof Error ? err.message : String(err) },
      { status: 500 }
    );
  }
}
