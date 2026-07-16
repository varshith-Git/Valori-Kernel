"use client";

import { useState, useMemo, useEffect, useRef } from "react";
import { RotateCcw, Code2, Copy, Check, Maximize2, Minimize2, ChevronDown, ChevronRight } from "lucide-react";
import { useHealth } from "@/lib/hooks/useHealth";
import { useProjects } from "@/lib/hooks/useProjects";

/* -- Endpoint catalog -------------------------------------------------- */

interface EndpointDef {
  name: string;
  method: "GET" | "POST" | "PATCH" | "DELETE";
  path: string;
  description: string;
  // Sample body as a function of dim (vector width) and the selected collection.
  sampleBody?: (dim: number, collection: string) => unknown;
}

// Substitutes the selected collection into a path's `?collection=` query
// param, whatever its current value. Paths with no collection param are
// returned unchanged (the regex simply doesn't match).
function withCollection(path: string, collection: string): string {
  return path.replace(/collection=[^&]*/, `collection=${encodeURIComponent(collection)}`);
}

interface EndpointGroup {
  label: string;
  endpoints: EndpointDef[];
}

const METHODS = ["GET", "POST", "PATCH", "DELETE"] as const;

const vec = (dim: number) => Array.from({ length: dim }, (_, i) => +(Math.sin(i + 1) * 0.1).toFixed(4));

function methodColor(method: string) {
  return method === "GET" ? "text-emerald-500" :
    method === "POST" ? "text-sky-500" :
    method === "PATCH" ? "text-amber-500" : "text-red-500";
}

const STATUS_TEXT: Record<number, string> = {
  200: "OK", 201: "Created", 204: "No Content",
  400: "Bad Request", 401: "Unauthorized", 403: "Forbidden", 404: "Not Found",
  409: "Conflict", 422: "Unprocessable Entity",
  500: "Internal Server Error", 503: "Service Unavailable",
};
function statusText(status: number) {
  return STATUS_TEXT[status] ?? (status < 300 ? "OK" : status < 500 ? "Error" : "Server Error");
}

// Lightweight JSON syntax highlighter for the read-only response pane.
// Escapes HTML first, so this is safe against values that contain markup.
function highlightJson(json: string): string {
  const escaped = json.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");
  return escaped.replace(
    /("(?:\\u[\da-fA-F]{4}|\\[^u]|[^\\"])*"(\s*:)?|\btrue\b|\bfalse\b|\bnull\b|-?\d+(?:\.\d+)?(?:[eE][+-]?\d+)?)/g,
    (match) => {
      let cls = "text-sky-500 dark:text-sky-400";
      if (match.startsWith('"')) {
        cls = /:\s*$/.test(match) ? "text-foreground/70" : "text-emerald-600 dark:text-emerald-400";
      } else if (match === "true" || match === "false") {
        cls = "text-amber-600 dark:text-amber-400";
      } else if (match === "null") {
        cls = "text-muted-foreground";
      }
      return `<span class="${cls}">${match}</span>`;
    }
  );
}

const CATALOG: EndpointGroup[] = [
  {
    label: "Data",
    endpoints: [
      {
        name: "Insert Record", method: "POST", path: "/v1/records",
        description: "Insert a single vector. Returns the new record id and state hash.",
        sampleBody: (dim, collection) => ({ values: vec(dim), collection }),
      },
      {
        name: "Batch Insert", method: "POST", path: "/v1/vectors/batch-insert",
        description: "Insert multiple vectors atomically — all succeed or the batch fails.",
        sampleBody: (dim, collection) => ({ batch: [vec(dim), vec(dim).map((v) => -v)], collection }),
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
        sampleBody: (dim, collection) => ({ query: vec(dim), k: 5, collection }),
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
        sampleBody: (dim, collection) => ({ query: vec(dim), k: 5, depth: 2, collection }),
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
  headers?: Record<string, string>;
}

export default function PlaygroundPage() {
  const { dim, online } = useHealth();
  const effectiveDim = dim ?? 8;
  const { projects } = useProjects();
  const collections = projects && projects.length > 0 ? projects : ["default"];

  const [collection, setCollection] = useState("default");
  const [selected, setSelected] = useState<EndpointDef>(CATALOG[0].endpoints[0]);
  const [method, setMethod] = useState<EndpointDef["method"]>(CATALOG[0].endpoints[0].method);
  const [path, setPath] = useState(CATALOG[0].endpoints[0].path);
  const [body, setBody] = useState(() =>
    JSON.stringify(CATALOG[0].endpoints[0].sampleBody?.(8, "default") ?? null, null, 2)
  );
  const [isSample, setIsSample] = useState(true);
  const [result, setResult] = useState<RunResult | null>(null);
  const [running, setRunning] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [copied, setCopied] = useState<"curl" | "response" | null>(null);
  const [headersOpen, setHeadersOpen] = useState(false);
  const [responseExpanded, setResponseExpanded] = useState(false);
  const [bodyExpanded, setBodyExpanded] = useState(false);
  const bodyGutterRef = useRef<HTMLDivElement>(null);
  const responseGutterRef = useRef<HTMLDivElement>(null);

  const [now, setNow] = useState(() => new Date());
  useEffect(() => {
    const id = setInterval(() => setNow(new Date()), 1000);
    return () => clearInterval(id);
  }, []);
  const timeStr = now.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit", second: "2-digit" });

  // The initial sample is generated before health loads (dim fallback = 8).
  // Once the real dim arrives, regenerate — but never clobber user edits.
  useEffect(() => {
    if (dim != null && isSample && selected.sampleBody) {
      setBody(JSON.stringify(selected.sampleBody(dim, collection), null, 2));
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [dim, selected]);

  // Switching the target collection re-points the current sample body and
  // path at it, without discarding manual edits elsewhere in either — path
  // substitution works on whatever collection value is already there, and
  // the body is only rewritten while it's still an unedited sample.
  useEffect(() => {
    setPath((p) => withCollection(p, collection));
    if (isSample && selected.sampleBody) {
      setBody(JSON.stringify(selected.sampleBody(dim ?? 8, collection), null, 2));
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [collection]);

  const pick = (ep: EndpointDef) => {
    setSelected(ep);
    setMethod(ep.method);
    setPath(withCollection(ep.path, collection));
    setBody(ep.sampleBody ? JSON.stringify(ep.sampleBody(dim ?? 8, collection), null, 2) : "");
    setIsSample(true);
    setResult(null);
    setError(null);
    setHeadersOpen(false);
    setResponseExpanded(false);
    setBodyExpanded(false);
    setCopied(null);
  };

  const resetSample = () => {
    if (!selected.sampleBody) return;
    setIsSample(true);
    setBody(JSON.stringify(selected.sampleBody(dim ?? 8, collection), null, 2));
  };

  const copyText = (text: string, which: "curl" | "response") => {
    navigator.clipboard.writeText(text);
    setCopied(which);
    setTimeout(() => setCopied((c) => (c === which ? null : c)), 1500);
  };

  const run = async () => {
    setRunning(true);
    setError(null);
    setResult(null);
    setHeadersOpen(false);
    setResponseExpanded(false);
    try {
      let parsedBody: unknown = undefined;
      if (method !== "GET" && body.trim()) {
        try { parsedBody = JSON.parse(body); } catch {
          throw new Error("Body is not valid JSON");
        }
      }
      const res = await fetch("/api/playground", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ method, path, body: parsedBody }),
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
    let cmd = `curl -X ${method} '${base}${path}'`;
    if (method !== "GET" && body.trim()) {
      cmd += ` \\\n  -H 'Content-Type: application/json' \\\n  -d '${body.replace(/\n\s*/g, " ")}'`;
    }
    return cmd;
  }, [method, path, body]);

  // Pull state_hash out of the response for the proof callout.
  const stateHash = useMemo(() => {
    const d = result?.data;
    if (d && typeof d === "object" && "state_hash" in d) {
      const h = (d as { state_hash?: unknown }).state_hash;
      return typeof h === "string" ? h : null;
    }
    return null;
  }, [result]);

  const responseText = useMemo(() => {
    if (!result) return "";
    return typeof result.data === "string" ? result.data : JSON.stringify(result.data, null, 2);
  }, [result]);
  const responseLineCount = useMemo(() => responseText.split("\n").length, [responseText]);
  const bodyLineCount = useMemo(() => body.split("\n").length, [body]);
  const headerEntries = useMemo(() => Object.entries(result?.headers ?? {}), [result]);

  return (
    <div className="p-6 max-w-[1400px]">
      <div className="mb-6 flex items-start justify-between gap-4">
        <div>
          <h1 className="text-xl font-semibold text-foreground">API Playground</h1>
          <p className="mt-1 text-sm text-muted-foreground">
            Run any node endpoint against the live server. Sample bodies use the node&apos;s vector dimension
            {dim != null && <span className="font-mono"> ({dim})</span>}.
            {!online && <span className="text-red-600 dark:text-red-400"> Node offline — requests will fail.</span>}
          </p>
        </div>
        <div className="flex items-center gap-3 shrink-0 pt-0.5">
          <div className="relative">
            <select
              value={collection}
              onChange={(e) => setCollection(e.target.value)}
              className="appearance-none rounded-lg border border-border bg-transparent pl-2.5 pr-7 py-1.5 text-xs font-mono text-foreground focus:outline-none focus:ring-1 focus:ring-[var(--v-accent-ring)] cursor-pointer"
            >
              {collections.map((c) => (
                <option key={c} value={c} className="bg-card text-foreground">{c}</option>
              ))}
            </select>
            <ChevronDown className="pointer-events-none absolute right-2 top-1/2 -translate-y-1/2 h-3 w-3 text-muted-foreground" aria-hidden />
          </div>
          <div className="flex items-center gap-1.5 text-xs text-muted-foreground">
          <span className="relative flex h-2 w-2">
            {online && <span className="animate-ping absolute inline-flex h-full w-full rounded-full bg-emerald-400 opacity-75" />}
            <span className={`relative inline-flex h-2 w-2 rounded-full ${online ? "bg-emerald-500" : "bg-red-500"}`} />
          </span>
          <span className={`font-medium ${online ? "text-emerald-600 dark:text-emerald-400" : "text-red-600 dark:text-red-400"}`}>
            {online ? "Live" : "Offline"}
          </span>
          <span className="text-muted-foreground/60">·</span>
          <span className="tabular-nums">{timeStr}</span>
          </div>
        </div>
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
                    <span className={`text-[10px] font-mono font-semibold w-11 shrink-0 ${methodColor(ep.method)}`}>{ep.method}</span>
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
            <div className="relative shrink-0">
              <select
                value={method}
                onChange={(e) => setMethod(e.target.value as EndpointDef["method"])}
                className={`appearance-none rounded-lg border border-border bg-transparent pl-2.5 pr-7 py-2 text-xs font-mono font-semibold focus:outline-none focus:ring-1 focus:ring-[var(--v-accent-ring)] ${methodColor(method)}`}
              >
                {METHODS.map((m) => (
                  <option key={m} value={m} className="bg-card text-foreground">{m}</option>
                ))}
              </select>
              <ChevronDown className="pointer-events-none absolute right-2 top-1/2 -translate-y-1/2 h-3 w-3 text-muted-foreground" aria-hidden />
            </div>
            <input
              value={path}
              onChange={(e) => setPath(e.target.value)}
              spellCheck={false}
              className="flex-1 rounded-lg border border-input bg-accent px-3 py-2 font-mono text-sm text-foreground focus:outline-none focus:ring-1 focus:ring-[var(--v-accent-ring)]"
            />
            <button
              onClick={run}
              disabled={running}
              className="flex items-center gap-1.5 rounded-lg bg-[var(--v-accent)] px-5 py-2 text-sm font-medium text-white hover:opacity-90 transition-opacity disabled:opacity-50"
            >
              {running ? "Running…" : "▶ Run"}
            </button>
          </div>

          {/* Content-Type + sample/curl actions */}
          <div className="flex items-center justify-between gap-2">
            <div>
              {method !== "GET" && (
                <div className="relative inline-block">
                  <select
                    value="application/json"
                    disabled
                    className="appearance-none rounded-lg border border-border bg-transparent pl-3 pr-7 py-1.5 text-xs font-mono text-muted-foreground disabled:cursor-default focus:outline-none"
                  >
                    <option value="application/json">Content-Type: application/json</option>
                  </select>
                  <ChevronDown className="pointer-events-none absolute right-2 top-1/2 -translate-y-1/2 h-3 w-3 text-muted-foreground/60" aria-hidden />
                </div>
              )}
            </div>
            <div className="flex items-center gap-2">
              {method !== "GET" && selected.sampleBody && (
                <button
                  onClick={resetSample}
                  className="flex items-center gap-1.5 rounded-lg border border-border bg-card px-2.5 py-1.5 text-xs font-medium text-muted-foreground hover:text-foreground hover:bg-accent/60 transition-colors"
                >
                  <RotateCcw className="h-3 w-3" aria-hidden />
                  Reset sample ({dim ?? 8}d)
                </button>
              )}
              <button
                onClick={() => copyText(curl, "curl")}
                className="flex items-center gap-1.5 rounded-lg border border-border bg-card px-2.5 py-1.5 text-xs font-medium text-muted-foreground hover:text-foreground hover:bg-accent/60 transition-colors"
              >
                {copied === "curl" ? <Check className="h-3 w-3 text-emerald-500" aria-hidden /> : <Code2 className="h-3 w-3" aria-hidden />}
                {copied === "curl" ? "Copied" : "Copy as cURL"}
              </button>
            </div>
          </div>

          {/* Body editor */}
          {method !== "GET" && (
            <div>
              <div className="flex items-center gap-2 mb-1.5">
                <label className="text-xs font-medium text-foreground">Request Body</label>
                {isSample && (
                  <span className="rounded-full border border-emerald-500/20 bg-emerald-500/10 px-2 py-0.5 text-[10px] font-medium text-emerald-600 dark:text-emerald-400">
                    Sample
                  </span>
                )}
              </div>
              <div className="relative flex rounded-lg border border-input bg-accent overflow-hidden">
                <div
                  ref={bodyGutterRef}
                  aria-hidden
                  className="shrink-0 select-none overflow-y-hidden py-3 pl-3 pr-2 text-right font-mono text-[11px] leading-[1.5rem] text-muted-foreground/40"
                  style={{ height: bodyExpanded ? "60vh" : "16rem" }}
                >
                  {Array.from({ length: bodyLineCount }, (_, i) => <div key={i}>{i + 1}</div>)}
                </div>
                <textarea
                  value={body}
                  onChange={(e) => { setIsSample(false); setBody(e.target.value); }}
                  onScroll={(e) => { if (bodyGutterRef.current) bodyGutterRef.current.scrollTop = e.currentTarget.scrollTop; }}
                  spellCheck={false}
                  className="flex-1 min-w-0 bg-transparent font-mono text-xs text-foreground py-3 pr-3 pl-2 resize-none focus:outline-none"
                  style={{ height: bodyExpanded ? "60vh" : "16rem", lineHeight: "1.5rem" }}
                />
                <button
                  onClick={() => setBodyExpanded((v) => !v)}
                  aria-label={bodyExpanded ? "Collapse request body" : "Expand request body"}
                  className="absolute right-2 top-2 flex h-5 w-5 items-center justify-center rounded-md border border-border bg-card text-muted-foreground hover:text-foreground hover:bg-accent/60 transition-colors"
                >
                  {bodyExpanded ? <Minimize2 className="h-2.5 w-2.5" aria-hidden /> : <Maximize2 className="h-2.5 w-2.5" aria-hidden />}
                </button>
              </div>
            </div>
          )}

          {error && (
            <div className="rounded-lg border border-red-500/30 bg-red-500/10 px-4 py-3 text-sm text-red-600 dark:text-red-400">
              {error}
            </div>
          )}

          {/* Response */}
          {result && (
            <div className="flex flex-col gap-2">
              <div className="flex items-center justify-between">
                <div className="flex items-center gap-3">
                  <p className="text-xs font-medium text-foreground">Response</p>
                  <span className={`text-xs font-mono px-2 py-0.5 rounded border ${
                    result.status < 300
                      ? "bg-emerald-500/10 text-emerald-500 border-emerald-500/40"
                      : "bg-red-500/10 text-red-500 border-red-500/40"
                  }`}>{result.status} {statusText(result.status)}</span>
                  <span className="text-muted-foreground/60">·</span>
                  <span className="text-xs font-mono text-muted-foreground">{result.latencyMs} ms</span>
                </div>
                <div className="flex items-center gap-1.5">
                  <button
                    onClick={() => copyText(responseText, "response")}
                    className="flex items-center gap-1.5 rounded-lg border border-border bg-card px-2 py-1 text-[11px] font-medium text-muted-foreground hover:text-foreground hover:bg-accent/60 transition-colors"
                  >
                    {copied === "response" ? <Check className="h-3 w-3 text-emerald-500" aria-hidden /> : <Copy className="h-3 w-3" aria-hidden />}
                    {copied === "response" ? "Copied" : "Copy"}
                  </button>
                  <button
                    onClick={() => setResponseExpanded((v) => !v)}
                    aria-label={responseExpanded ? "Collapse response" : "Expand response"}
                    className="flex h-6 w-6 items-center justify-center rounded-lg border border-border bg-card text-muted-foreground hover:text-foreground hover:bg-accent/60 transition-colors"
                  >
                    {responseExpanded ? <Minimize2 className="h-3 w-3" aria-hidden /> : <Maximize2 className="h-3 w-3" aria-hidden />}
                  </button>
                </div>
              </div>

              {stateHash && (
                <div className="rounded-lg border border-[var(--v-accent-ring)] bg-[var(--v-accent-muted)] px-3 py-2">
                  <p className="text-[11px] text-muted-foreground">state_hash — BLAKE3 root over all applied events; reproducible from the audit log</p>
                  <p className="font-mono text-xs text-foreground break-all mt-0.5">{stateHash}</p>
                </div>
              )}

              <div className="flex rounded-lg border border-border bg-accent overflow-hidden">
                <div
                  ref={responseGutterRef}
                  aria-hidden
                  className="shrink-0 select-none overflow-y-hidden py-3 pl-3 pr-2 text-right font-mono text-[11px] leading-[1.5rem] text-muted-foreground/40"
                  style={{ maxHeight: responseExpanded ? "70vh" : "420px" }}
                >
                  {Array.from({ length: responseLineCount }, (_, i) => <div key={i}>{i + 1}</div>)}
                </div>
                <pre
                  onScroll={(e) => { if (responseGutterRef.current) responseGutterRef.current.scrollTop = e.currentTarget.scrollTop; }}
                  className="flex-1 min-w-0 overflow-x-auto overflow-y-auto py-3 pr-3 pl-2 font-mono text-xs whitespace-pre-wrap break-all"
                  style={{ maxHeight: responseExpanded ? "70vh" : "420px", lineHeight: "1.5rem" }}
                  dangerouslySetInnerHTML={{ __html: highlightJson(responseText) }}
                />
              </div>

              {headerEntries.length > 0 && (
                <div className="rounded-lg border border-border overflow-hidden">
                  <button
                    onClick={() => setHeadersOpen((o) => !o)}
                    className="flex w-full items-center gap-1.5 px-3 py-2 text-xs font-medium text-muted-foreground hover:text-foreground transition-colors"
                  >
                    {headersOpen ? <ChevronDown className="h-3.5 w-3.5" aria-hidden /> : <ChevronRight className="h-3.5 w-3.5" aria-hidden />}
                    Headers ({headerEntries.length})
                  </button>
                  {headersOpen && (
                    <div className="border-t border-border px-3 py-2 flex flex-col gap-1">
                      {headerEntries.map(([k, v]) => (
                        <div key={k} className="flex gap-2 font-mono text-[11px]">
                          <span className="text-muted-foreground shrink-0">{k}:</span>
                          <span className="text-foreground break-all">{v}</span>
                        </div>
                      ))}
                    </div>
                  )}
                </div>
              )}
            </div>
          )}
        </div>
      </div>
    </div>
  );
}
