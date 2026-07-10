"use client";

import { useState, useMemo, useEffect, useRef } from "react";
import { useHealth } from "@/lib/hooks/useHealth";

/* -- Endpoint catalog -------------------------------------------------- */

interface EndpointDef {
  name: string;
  method: "GET" | "POST" | "PATCH" | "DELETE";
  path: string;
  description: string;
  // Sample body as a function of dim so vectors match the node's dimension.
  sampleBody?: (dim: number) => unknown;
}

interface EndpointGroup {
  label: string;
  endpoints: EndpointDef[];
}

const vec = (dim: number) => Array.from({ length: dim }, (_, i) => +(Math.sin(i + 1) * 0.1).toFixed(4));

const CATALOG: EndpointGroup[] = [
  {
    label: "Data",
    endpoints: [
      {
        name: "Insert Record", method: "POST", path: "/v1/records",
        description: "Insert a single vector. Returns the new record id and state hash.",
        sampleBody: (dim) => ({ values: vec(dim), collection: "default" }),
      },
      {
        name: "Batch Insert", method: "POST", path: "/v1/vectors/batch-insert",
        description: "Insert multiple vectors atomically — all succeed or the batch fails.",
        sampleBody: (dim) => ({ batch: [vec(dim), vec(dim).map((v) => -v)], collection: "default" }),
      },
      {
        name: "Get Record", method: "GET", path: "/v1/records/0?collection=default",
        description: "Fetch one record by id: vector, metadata, and tag.",
      },
      {
        name: "Update Metadata", method: "PATCH", path: "/v1/records/0/metadata?collection=default",
        description: "Replace the JSON payload on an existing record. Audited like any other write.",
        sampleBody: () => ({ author: "alice", year: 2026 }),
      },
      {
        name: "Search", method: "POST", path: "/v1/search",
        description: "K-nearest-neighbour search. Score is L2² distance — smaller is closer.",
        sampleBody: (dim) => ({ query: vec(dim), k: 5, collection: "default" }),
      },
      {
        name: "Soft Delete", method: "POST", path: "/v1/soft-delete",
        description: "Tombstone a record — excluded from search but the audit history is preserved.",
        sampleBody: () => ({ id: 0 }),
      },
    ],
  },
  {
    label: "Collections",
    endpoints: [
      {
        name: "List Collections", method: "GET", path: "/v1/namespaces",
        description: "All collections (namespaces) with their integer ids.",
      },
      {
        name: "Create Collection", method: "POST", path: "/v1/namespaces",
        description: "Create a named collection. Data is fully isolated between collections.",
        sampleBody: () => ({ name: "tenant-acme" }),
      },
    ],
  },
  {
    label: "Graph + RAG",
    endpoints: [
      {
        name: "GraphRAG", method: "POST", path: "/v1/graphrag",
        description: "K nearest vectors plus the connected knowledge-graph subgraph in one call.",
        sampleBody: (dim) => ({ query: vec(dim), k: 5, depth: 2, collection: "default" }),
      },
      {
        name: "Create Graph Node", method: "POST", path: "/v1/graph/node",
        description: "Add a node to the knowledge graph, optionally linked to a record. kind is a u8 (0=Document, 1=Chunk, 2=Entity…).",
        sampleBody: () => ({ kind: 2, record_id: null }),
      },
    ],
  },
  {
    label: "Agent Memory",
    endpoints: [
      {
        name: "Memory Upsert", method: "POST", path: "/v1/memory/upsert",
        description: "Insert a memory: record + document/chunk graph nodes in one call.",
        sampleBody: (dim) => ({ vector: vec(dim), metadata: { role: "note" } }),
      },
      {
        name: "Memory Search", method: "POST", path: "/v1/memory/search_vector",
        description: "Vector search with optional recency decay (decay_half_life_secs).",
        sampleBody: (dim) => ({ query_vector: vec(dim), k: 5, decay_half_life_secs: 86400 }),
      },
      {
        name: "Consolidate", method: "POST", path: "/v1/memory/consolidate",
        description: "Soft-delete an old memory, insert its replacement, and commit a Supersedes edge to the audit chain.",
        sampleBody: (dim) => ({ old_record_id: 0, new_vector: vec(dim) }),
      },
      {
        name: "Contradict", method: "POST", path: "/v1/memory/contradict",
        description: "Check two records for contradiction (cosine ≥ threshold) and record a Contradicts edge.",
        sampleBody: () => ({ record_a: 0, record_b: 1, threshold: 0.9 }),
      },
    ],
  },
  {
    label: "Proof",
    endpoints: [
      {
        name: "Event-Log Proof", method: "GET", path: "/v1/proof/event-log",
        description: "The receipt primitive: BLAKE3 event-log hash, final state hash, and committed height.",
      },
      {
        name: "State Proof", method: "GET", path: "/v1/proof/state",
        description: "Current state hash — the BLAKE3 Merkle root over all applied events.",
      },
      {
        name: "Timeline", method: "GET", path: "/v1/timeline",
        description: "Chronological audit trail of every committed event with hashes.",
      },
    ],
  },
  {
    label: "Node",
    endpoints: [
      { name: "Health", method: "GET", path: "/health", description: "Node status, dimension, record counts, index kind." },
      { name: "Version", method: "GET", path: "/v1/version", description: "Build version and feature flags." },
    ],
  },
];

/* -- Page --------------------------------------------------------------- */

interface RunResult {
  status: number;
  latencyMs: number;
  data: unknown;
}

export default function PlaygroundPage() {
  const { dim, online } = useHealth();
  const effectiveDim = dim ?? 8;

  const [selected, setSelected] = useState<EndpointDef>(CATALOG[0].endpoints[0]);
  const [path, setPath] = useState(CATALOG[0].endpoints[0].path);
  const [body, setBody] = useState(() =>
    JSON.stringify(CATALOG[0].endpoints[0].sampleBody?.(8) ?? null, null, 2)
  );
  const [result, setResult] = useState<RunResult | null>(null);
  const [running, setRunning] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const bodyTouched = useRef(false);

  // The initial sample is generated before health loads (dim fallback = 8).
  // Once the real dim arrives, regenerate — but never clobber user edits.
  useEffect(() => {
    if (dim != null && !bodyTouched.current && selected.sampleBody) {
      setBody(JSON.stringify(selected.sampleBody(dim), null, 2));
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [dim, selected]);

  const pick = (ep: EndpointDef) => {
    setSelected(ep);
    setPath(ep.path);
    setBody(ep.sampleBody ? JSON.stringify(ep.sampleBody(dim ?? 8), null, 2) : "");
    setResult(null);
    setError(null);
    bodyTouched.current = false;
  };

  const run = async () => {
    setRunning(true);
    setError(null);
    setResult(null);
    try {
      let parsedBody: unknown = undefined;
      if (selected.method !== "GET" && body.trim()) {
        try { parsedBody = JSON.parse(body); } catch {
          throw new Error("Body is not valid JSON");
        }
      }
      const res = await fetch("/api/playground", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ method: selected.method, path, body: parsedBody }),
      });
      const payload = await res.json();
      if (!res.ok) throw new Error(payload.error ?? `Request failed (${res.status})`);
      setResult(payload as RunResult);
    } catch (e) {
      setError(e instanceof Error ? e.message : "Request failed");
    } finally {
      setRunning(false);
    }
  };

  const curl = useMemo(() => {
    const base = "$VALORI_URL";
    let cmd = `curl -X ${selected.method} '${base}${path}'`;
    if (selected.method !== "GET" && body.trim()) {
      cmd += ` \\\n  -H 'Content-Type: application/json' \\\n  -d '${body.replace(/\n\s*/g, " ")}'`;
    }
    return cmd;
  }, [selected.method, path, body]);

  // Pull state_hash out of the response for the proof callout.
  const stateHash = useMemo(() => {
    const d = result?.data;
    if (d && typeof d === "object" && "state_hash" in d) {
      const h = (d as { state_hash?: unknown }).state_hash;
      return typeof h === "string" ? h : null;
    }
    return null;
  }, [result]);

  return (
    <div className="p-6 max-w-[1400px]">
      <div className="mb-6">
        <h1 className="text-xl font-semibold text-foreground">API Playground</h1>
        <p className="mt-1 text-sm text-muted-foreground">
          Run any node endpoint against the live server. Sample bodies use the node&apos;s vector dimension
          {dim != null && <span className="font-mono"> ({dim})</span>}.
          {!online && <span className="text-red-600 dark:text-red-400"> Node offline — requests will fail.</span>}
        </p>
      </div>

      <div className="flex gap-6">
        {/* Endpoint catalog */}
        <div className="w-64 shrink-0 flex flex-col gap-4">
          {CATALOG.map((group) => (
            <div key={group.label}>
              <p className="text-[11px] uppercase tracking-wider text-muted-foreground mb-1.5 px-1">{group.label}</p>
              <div className="flex flex-col">
                {group.endpoints.map((ep) => (
                  <button
                    key={ep.method + ep.path + ep.name}
                    onClick={() => pick(ep)}
                    className={`flex items-center gap-2 text-left px-2.5 py-1.5 rounded-lg text-sm transition-colors ${
                      selected === ep
                        ? "bg-[var(--v-accent-muted)] text-foreground"
                        : "text-muted-foreground hover:bg-accent hover:text-foreground"
                    }`}
                  >
                    <span className={`text-[10px] font-mono font-semibold w-11 shrink-0 ${
                      ep.method === "GET" ? "text-emerald-500" :
                      ep.method === "POST" ? "text-sky-500" :
                      ep.method === "PATCH" ? "text-amber-500" : "text-red-500"
                    }`}>{ep.method}</span>
                    <span className="truncate">{ep.name}</span>
                  </button>
                ))}
              </div>
            </div>
          ))}
        </div>

        {/* Request + response */}
        <div className="flex-1 min-w-0 flex flex-col gap-4">
          <p className="text-sm text-muted-foreground -mb-1">{selected.description}</p>

          {/* Method + path + run */}
          <div className="flex items-center gap-2">
            <span className={`text-xs font-mono font-semibold px-2.5 py-2 rounded-lg border border-border ${
              selected.method === "GET" ? "text-emerald-500" :
              selected.method === "POST" ? "text-sky-500" :
              selected.method === "PATCH" ? "text-amber-500" : "text-red-500"
            }`}>{selected.method}</span>
            <input
              value={path}
              onChange={(e) => setPath(e.target.value)}
              spellCheck={false}
              className="flex-1 rounded-lg border border-input bg-accent px-3 py-2 font-mono text-sm text-foreground focus:outline-none focus:ring-1 focus:ring-[var(--v-accent-ring)]"
            />
            <button
              onClick={run}
              disabled={running}
              className="rounded-lg bg-[var(--v-accent)] px-5 py-2 text-sm font-medium text-white hover:opacity-90 transition-opacity disabled:opacity-50"
            >
              {running ? "Running…" : "▶ Run"}
            </button>
          </div>

          {/* Body editor */}
          {selected.method !== "GET" && (
            <div>
              <div className="flex items-center justify-between mb-1.5">
                <label className="text-xs text-muted-foreground">Body <span className="font-mono">JSON</span></label>
                <div className="flex items-center gap-2">
                  {selected.sampleBody && (
                    <button
                      onClick={() => {
                        bodyTouched.current = false;
                        setBody(JSON.stringify(selected.sampleBody?.(dim ?? 8), null, 2));
                      }}
                      className="text-[11px] font-mono px-2 py-0.5 rounded border border-input text-muted-foreground hover:border-emerald-600 hover:text-emerald-400 transition-all"
                    >
                      reset sample ({dim ?? 8}d)
                    </button>
                  )}
                  <button
                    onClick={() => navigator.clipboard.writeText(curl)}
                    className="text-[11px] font-mono px-2 py-0.5 rounded border border-input text-muted-foreground hover:border-sky-600 hover:text-sky-400 transition-all"
                  >
                    copy as curl
                  </button>
                </div>
              </div>
              <textarea
                value={body}
                onChange={(e) => { bodyTouched.current = true; setBody(e.target.value); }}
                rows={10}
                spellCheck={false}
                className="w-full rounded-lg border border-input bg-accent font-mono text-xs text-foreground p-3 resize-y focus:outline-none focus:ring-1 focus:ring-[var(--v-accent-ring)]"
              />
            </div>
          )}
          {selected.method === "GET" && (
            <div className="flex justify-end">
              <button
                onClick={() => navigator.clipboard.writeText(curl)}
                className="text-[11px] font-mono px-2 py-0.5 rounded border border-input text-muted-foreground hover:border-sky-600 hover:text-sky-400 transition-all"
              >
                copy as curl
              </button>
            </div>
          )}

          {error && (
            <div className="rounded-lg border border-red-800/50 bg-red-950/20 px-4 py-3 text-sm text-red-400">
              {error}
            </div>
          )}

          {/* Response */}
          {result && (
            <div className="flex flex-col gap-2">
              <div className="flex items-center gap-3">
                <p className="text-xs text-muted-foreground">Result</p>
                <span className={`text-xs font-mono px-2 py-0.5 rounded border ${
                  result.status < 300
                    ? "bg-emerald-500/10 text-emerald-500 border-emerald-500/40"
                    : "bg-red-500/10 text-red-500 border-red-500/40"
                }`}>{result.status}</span>
                <span className="text-xs font-mono text-muted-foreground">{result.latencyMs} ms</span>
              </div>
              {stateHash && (
                <div className="rounded-lg border border-[var(--v-accent-ring)] bg-[var(--v-accent-muted)] px-3 py-2">
                  <p className="text-[11px] text-muted-foreground">state_hash — BLAKE3 root over all applied events; reproducible from the audit log</p>
                  <p className="font-mono text-xs text-foreground break-all mt-0.5">{stateHash}</p>
                </div>
              )}
              <pre className="rounded-lg border border-border bg-accent p-3 font-mono text-xs text-foreground overflow-x-auto max-h-[420px] overflow-y-auto whitespace-pre-wrap break-all">
                {typeof result.data === "string" ? result.data : JSON.stringify(result.data, null, 2)}
              </pre>
            </div>
          )}
        </div>
      </div>
    </div>
  );
}
