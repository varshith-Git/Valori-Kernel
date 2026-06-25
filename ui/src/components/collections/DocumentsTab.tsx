"use client";

import { useState, useEffect, useCallback, useRef } from "react";
import useSWR, { useSWRConfig } from "swr";

const fetcher = (url: string) => fetch(url).then((r) => r.json());

interface GraphNode {
  node_id: number;
  kind: number;
  record_id: number | null;
}

interface DocMeta {
  filename?: string;
  file_size?: number;
  total_chunks?: number;
  provider?: string;
  model?: string;
  ingested_at?: string;
}

interface ChunkMeta {
  record_id: number;
  chunk_index: number;
  total_chunks: number;
  text: string;
  source: string;
}

// -- Copy button ---------------------------------------------------------------
function CopyBtn({ text, label = "copy" }: { text: string; label?: string }) {
  const [copied, setCopied] = useState(false);
  const copy = async () => {
    await navigator.clipboard.writeText(text);
    setCopied(true);
    setTimeout(() => setCopied(false), 1800);
  };
  return (
    <button
      onClick={copy}
      className={`inline-flex items-center gap-1 text-[10px] px-2 py-0.5 rounded border transition-all ${
        copied
          ? "border-emerald-700 bg-emerald-950/50 text-emerald-400"
          : "border-input bg-card text-muted-foreground hover:text-accent-foreground hover:border-ring"
      }`}
    >
      {copied ? "✓ copied" : label}
    </button>
  );
}

// -- Single chunk --------------------------------------------------------------
function ChunkCard({ chunk }: { chunk: ChunkMeta }) {
  const [expanded, setExpanded] = useState(false);
  const preview = chunk.text.slice(0, 280);
  const isLong = chunk.text.length > 280;

  return (
    <div className="group relative border-b border-border/50 last:border-0 px-5 py-4 hover:bg-accent/20 transition-colors">
      <div className="flex items-start gap-3">
        {/* Chunk number */}
        <span className="flex-shrink-0 w-8 text-right font-mono text-[10px] text-muted-foreground mt-0.5 select-none">
          {chunk.chunk_index + 1}
        </span>

        {/* Text */}
        <div className="flex-1 min-w-0">
          <p className="text-sm text-accent-foreground leading-relaxed whitespace-pre-wrap break-words">
            {expanded ? chunk.text : preview}
            {!expanded && isLong && (
              <span className="text-muted-foreground">…</span>
            )}
          </p>
          {isLong && (
            <button
              onClick={() => setExpanded((v) => !v)}
              className="mt-1.5 text-[10px] text-muted-foreground hover:text-muted-foreground transition-colors"
            >
              {expanded ? "show less ▲" : `show all ${chunk.text.length.toLocaleString()} chars ▼`}
            </button>
          )}
        </div>

        {/* Copy — visible on hover */}
        <div className="flex-shrink-0 opacity-0 group-hover:opacity-100 transition-opacity">
          <CopyBtn text={chunk.text} />
        </div>
      </div>
    </div>
  );
}

// -- Document card -------------------------------------------------------------
function DocumentCard({
  nodeId,
  meta,
  namespace,
  onDeleted,
}: {
  nodeId: number;
  meta: DocMeta;
  namespace: string;
  onDeleted: (id: number) => void;
}) {
  const [open, setOpen] = useState(false);
  const [chunks, setChunks] = useState<ChunkMeta[] | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [deleting, setDeleting] = useState(false);
  const [confirmDelete, setConfirmDelete] = useState(false);
  const confirmTimer = useRef<ReturnType<typeof setTimeout> | null>(null);

  const loadChunks = useCallback(async () => {
    if (chunks !== null) return;
    setLoading(true);
    setError(null);
    try {
      // 1. Get chunk node IDs from document edges
      const edgesRes = await fetch(`/api/graph/edges/${nodeId}`);
      if (!edgesRes.ok) throw new Error(`edges ${edgesRes.status}`);
      const edgesData = await edgesRes.json() as { edges?: { to_node: number }[] };
      const edgeList = edgesData.edges ?? [];

      // 2. Get all nodes in namespace to map chunk_node_id → record_id
      const nodesRes = await fetch(`/api/graph/nodes?collection=${encodeURIComponent(namespace)}`);
      const nodesData = nodesRes.ok
        ? await nodesRes.json() as { nodes?: GraphNode[] }
        : { nodes: [] };
      const nodeToRecord = new Map<number, number>();
      for (const n of nodesData.nodes ?? []) {
        if (n.record_id !== null) nodeToRecord.set(n.node_id, n.record_id);
      }

      // 3. Fetch metadata for each chunk (in parallel, cap at 100)
      const chunkResults: ChunkMeta[] = [];
      await Promise.all(
        edgeList.slice(0, 100).map(async (e) => {
          const rid = nodeToRecord.get(e.to_node);
          if (rid === undefined) return;
          const mr = await fetch(`/api/meta?target_id=record:${rid}`);
          if (!mr.ok) return;
          const d = await mr.json().catch(() => ({})) as { metadata?: Record<string, unknown> };
          const m = d.metadata;
          if (!m?.text) return;
          chunkResults.push({
            record_id: rid,
            chunk_index: (m.chunk_index as number) ?? 0,
            total_chunks: (m.total_chunks as number) ?? 0,
            text: m.text as string,
            source: (m.source as string) ?? meta.filename ?? "",
          });
        })
      );

      setChunks(chunkResults.sort((a, b) => a.chunk_index - b.chunk_index));
    } catch (e) {
      setError(e instanceof Error ? e.message : "Failed to load chunks");
      setChunks([]);
    } finally {
      setLoading(false);
    }
  }, [nodeId, namespace, chunks, meta.filename]);

  const toggle = async () => {
    if (!open) await loadChunks();
    setOpen((v) => !v);
  };

  const handleDelete = async (e: React.MouseEvent) => {
    e.stopPropagation();
    if (!confirmDelete) {
      setConfirmDelete(true);
      confirmTimer.current = setTimeout(() => setConfirmDelete(false), 3000);
      return;
    }
    if (confirmTimer.current) clearTimeout(confirmTimer.current);
    setDeleting(true);
    try {
      const res = await fetch(
        `/api/documents/${nodeId}?collection=${encodeURIComponent(namespace)}`,
        { method: "DELETE" }
      );
      if (!res.ok) {
        const d = await res.json().catch(() => ({})) as { error?: string };
        throw new Error(d.error ?? `HTTP ${res.status}`);
      }
      onDeleted(nodeId);
    } catch (err) {
      setError(err instanceof Error ? err.message : "Delete failed");
      setDeleting(false);
      setConfirmDelete(false);
    }
  };

  const fullText = chunks?.map((c) => c.text).join("\n\n") ?? "";
  const filename = meta.filename ?? `Document ${nodeId}`;
  const ext = filename.split(".").pop()?.toUpperCase() ?? "";

  return (
    <div className="relative rounded-xl border border-border bg-card overflow-hidden">
      {/* Header row */}
      <button
        onClick={toggle}
        className="w-full flex items-center gap-4 px-5 py-4 text-left hover:bg-accent/40 transition-colors pr-28"
      >
        {/* File type badge */}
        <div className="flex-shrink-0 w-10 h-10 rounded-lg bg-accent border border-input flex items-center justify-center">
          <span className="text-[9px] font-bold text-muted-foreground">{ext || "DOC"}</span>
        </div>

        <div className="flex-1 min-w-0">
          <p className="text-sm font-medium text-foreground truncate">{filename}</p>
          <div className="flex items-center gap-3 mt-0.5 flex-wrap">
            <span className="text-[11px] text-muted-foreground">
              {meta.total_chunks ?? "?"} chunks
            </span>
            {meta.file_size && (
              <span className="text-[11px] text-muted-foreground">
                {(meta.file_size / 1024).toFixed(0)} KB
              </span>
            )}
            {meta.ingested_at && (
              <span className="text-[11px] text-muted-foreground">
                {new Date(meta.ingested_at).toLocaleDateString(undefined, {
                  month: "short", day: "numeric", year: "numeric",
                })}
              </span>
            )}
            {meta.provider && (
              <span className="text-[10px] font-mono text-muted-foreground">
                {meta.provider}/{meta.model}
              </span>
            )}
          </div>
        </div>

        <span className="text-muted-foreground text-xs flex-shrink-0 ml-2">
          {open ? "▲" : "▼"}
        </span>

        {/* Delete button — outside the toggle button to avoid nested buttons */}
      </button>
      {/* Delete control — rendered outside the toggle button */}
      <div className="absolute top-3.5 right-12 flex items-center gap-1.5">
        {error && !deleting && (
          <span className="text-[10px] text-red-500">{error}</span>
        )}
        <button
          onClick={handleDelete}
          disabled={deleting}
          title={confirmDelete ? "Click again to confirm delete" : "Delete document"}
          className={`text-[10px] px-2 py-1 rounded border transition-all ${
            confirmDelete
              ? "border-red-700 bg-red-950/60 text-red-400 animate-pulse"
              : "border-border text-muted-foreground hover:border-red-800 hover:text-red-500 hover:bg-red-950/30"
          } disabled:opacity-40`}
        >
          {deleting ? "deleting…" : confirmDelete ? "confirm?" : "delete"}
        </button>
      </div>

      {/* Expanded reader */}
      {open && (
        <div className="border-t border-border">
          {loading && (
            <div className="flex items-center gap-2.5 px-5 py-8 text-xs text-muted-foreground">
              <span className="h-3 w-3 animate-spin rounded-full border-2 border-muted border-t-zinc-300" />
              Loading {meta.total_chunks ?? ""} chunks…
            </div>
          )}

          {error && (
            <p className="px-5 py-4 text-xs text-red-500">{error}</p>
          )}

          {!loading && !error && chunks !== null && chunks.length === 0 && (
            <p className="px-5 py-6 text-xs text-muted-foreground">
              No chunk text found. Re-upload the document to populate metadata.
            </p>
          )}

          {!loading && chunks && chunks.length > 0 && (
            <>
              {/* Toolbar */}
              <div className="flex items-center justify-between px-5 py-2.5 bg-background/60 border-b border-border">
                <span className="text-[11px] text-muted-foreground">
                  {chunks.length} chunks · {(fullText.length / 1000).toFixed(1)}k chars extracted
                </span>
                <CopyBtn text={fullText} label="copy all text" />
              </div>

              {/* Chunk list */}
              <div className="max-h-[70vh] overflow-y-auto">
                {chunks.map((c) => (
                  <ChunkCard key={c.record_id} chunk={c} />
                ))}
              </div>
            </>
          )}
        </div>
      )}
    </div>
  );
}

// -- Tab root ------------------------------------------------------------------
export function DocumentsTab({ namespace }: { namespace: string }) {
  const { mutate } = useSWRConfig();
  const swrKey = `/api/graph/nodes?collection=${encodeURIComponent(namespace)}`;
  const { data, isLoading } = useSWR<{ nodes: GraphNode[] }>(swrKey, fetcher, {
    refreshInterval: 15000,
  });

  const [deleted, setDeleted] = useState<Set<number>>(new Set());
  const docNodes = (data?.nodes ?? [])
    .filter((n) => n.kind === 0)
    .filter((n) => !deleted.has(n.node_id));

  const handleDeleted = useCallback((id: number) => {
    setDeleted((prev) => new Set([...prev, id]));
    mutate(swrKey);
  }, [mutate, swrKey]);

  const [docMetas, setDocMetas] = useState<Map<number, DocMeta>>(new Map());

  // Load document-level metadata whenever the node list changes
  useEffect(() => {
    if (docNodes.length === 0) return;
    const missing = docNodes.filter((n) => !docMetas.has(n.node_id));
    if (missing.length === 0) return;

    Promise.all(
      missing.map(async (n) => {
        const res = await fetch(`/api/meta?target_id=document:${n.node_id}`);
        if (!res.ok) return [n.node_id, {}] as [number, DocMeta];
        const d = await res.json().catch(() => ({})) as { metadata?: DocMeta };
        return [n.node_id, d.metadata ?? {}] as [number, DocMeta];
      })
    ).then((entries) => {
      setDocMetas((prev) => {
        const next = new Map(prev);
        for (const [id, m] of entries) next.set(id, m);
        return next;
      });
    });
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [docNodes.length, namespace]);

  if (isLoading) {
    return (
      <div className="flex items-center gap-2.5 py-10 text-xs text-muted-foreground">
        <span className="h-3 w-3 animate-spin rounded-full border-2 border-muted border-t-zinc-300" />
        Loading documents…
      </div>
    );
  }

  if (docNodes.length === 0) {
    return (
      <div className="rounded-xl border border-border bg-card px-6 py-12 text-center">
        <p className="text-accent-foreground text-sm font-medium">No documents yet</p>
        <p className="text-muted-foreground text-xs mt-1.5 max-w-xs mx-auto">
          Upload a PDF, DOCX, or TXT file in the Upload tab. The content will appear here once ingested.
        </p>
      </div>
    );
  }

  return (
    <div className="flex flex-col gap-4">
      <p className="text-[11px] text-muted-foreground">
        {docNodes.length} document{docNodes.length !== 1 ? "s" : ""} · click to read
      </p>

      {docNodes.map((n) => (
        <DocumentCard
          key={n.node_id}
          nodeId={n.node_id}
          meta={docMetas.get(n.node_id) ?? {}}
          namespace={namespace}
          onDeleted={handleDeleted}
        />
      ))}
    </div>
  );
}
