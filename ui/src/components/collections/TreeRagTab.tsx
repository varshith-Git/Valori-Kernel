"use client";

import { useState } from "react";
import { FileText, Search, Zap, ChevronRight, Hash, Link2 } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Textarea } from "@/components/ui/textarea";
import { Input } from "@/components/ui/input";
import { Badge } from "@/components/ui/badge";
import { cn } from "@/lib/utils";

// ── Types mirroring the Rust structs ────────────────────────────────────────

interface StructureNode {
  id: string;
  title: string;
  depth: number;
  child_count: number;
}

interface BuildResult {
  cache_key: string;
  doc_name: string;
  node_count: number;
  structure_map: StructureNode[];
  error?: string;
}

interface Citation {
  node_id: string;
  title: string;
  breadcrumb: string;
  lines: [number, number];
}

interface Receipt {
  query: string;
  query_hash: string;
  answer_hash: string;
  prev_hash: string;
  receipt_hash: string;
  hash_algo: string;
  timestamp: number;
  visited_node_ids: string[];
  fetched_ranges: [number, number][];
}

interface AnswerResult {
  query: string;
  answer: string;
  citations: Citation[];
  evidence_text: string;
  reasoning: string;
  receipt: Receipt;
  error?: string;
}

// ── Component ────────────────────────────────────────────────────────────────

export function TreeRagTab({ namespace }: { namespace: string }) {
  const [text, setText] = useState("");
  const [docName, setDocName] = useState("document");
  const [buildResult, setBuildResult] = useState<BuildResult | null>(null);
  const [building, setBuilding] = useState(false);
  const [buildError, setBuildError] = useState<string | null>(null);

  const [query, setQuery] = useState("");
  const [k, setK] = useState(3);
  const [queryResult, setQueryResult] = useState<AnswerResult | null>(null);
  const [querying, setQuerying] = useState(false);
  const [queryError, setQueryError] = useState<string | null>(null);

  const [prevHash, setPrevHash] = useState<string | undefined>(undefined);

  async function handleBuild() {
    if (!text.trim()) return;
    setBuilding(true);
    setBuildError(null);
    setBuildResult(null);
    setQueryResult(null);
    setPrevHash(undefined);
    try {
      const res = await fetch("/api/tree/build", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ text, doc_name: docName || "document" }),
      });
      const data: BuildResult = await res.json();
      if (!res.ok || data.error) {
        setBuildError(data.error ?? `HTTP ${res.status}`);
      } else {
        setBuildResult(data);
      }
    } catch (e) {
      setBuildError(String(e));
    } finally {
      setBuilding(false);
    }
  }

  async function handleQuery() {
    if (!buildResult || !query.trim()) return;
    setQuerying(true);
    setQueryError(null);
    try {
      const res = await fetch("/api/tree/query", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({
          cache_key: buildResult.cache_key,
          query,
          k,
          prev_hash: prevHash,
        }),
      });
      const data: AnswerResult = await res.json();
      if (!res.ok || data.error) {
        setQueryError(data.error ?? `HTTP ${res.status}`);
      } else {
        setQueryResult(data);
        // Chain receipts: next query's prev_hash = this query's receipt_hash
        setPrevHash(data.receipt.receipt_hash);
      }
    } catch (e) {
      setQueryError(String(e));
    } finally {
      setQuerying(false);
    }
  }

  const hasTree = !!buildResult;

  return (
    <div className="flex flex-col gap-6 max-w-4xl">

      {/* Step 1 — Document input */}
      <section className="rounded-xl border border-border bg-card p-5 flex flex-col gap-4">
        <div className="flex items-center gap-2">
          <FileText size={16} className="text-[var(--v-accent)]" />
          <h3 className="text-sm font-semibold">1 — Paste your document</h3>
          <span className="text-xs text-muted-foreground ml-auto">
            Markdown headers (## Section) become the ToC tree nodes
          </span>
        </div>

        <div className="flex gap-2">
          <Input
            value={docName}
            onChange={(e) => setDocName(e.target.value)}
            placeholder="Document name"
            className="max-w-[200px] h-8 text-sm"
          />
        </div>

        <Textarea
          value={text}
          onChange={(e) => setText(e.target.value)}
          placeholder="Paste markdown text here — sections separated by ## headings work best"
          className="min-h-[200px] font-mono text-xs resize-y"
        />

        <div className="flex items-center gap-3">
          <Button
            size="sm"
            onClick={handleBuild}
            disabled={building || !text.trim()}
            className="gap-2"
          >
            <Zap size={14} />
            {building ? "Building…" : "Build Tree Index"}
          </Button>
          {buildError && (
            <p className="text-sm text-red-500 dark:text-red-400">{buildError}</p>
          )}
        </div>
      </section>

      {/* Step 2 — Tree structure (shown after build) */}
      {buildResult && (
        <section className="rounded-xl border border-border bg-card p-5 flex flex-col gap-4">
          <div className="flex items-center gap-2">
            <Hash size={16} className="text-[var(--v-accent)]" />
            <h3 className="text-sm font-semibold">2 — Tree structure</h3>
            <Badge variant="secondary" className="ml-auto text-xs">
              {buildResult.node_count} sections
            </Badge>
          </div>

          <div className="flex flex-col gap-0.5 max-h-48 overflow-y-auto">
            {buildResult.structure_map.map((node) => (
              <div
                key={node.id}
                className="flex items-center gap-2 py-0.5 text-sm"
                style={{ paddingLeft: `${node.depth * 16 + 4}px` }}
              >
                <ChevronRight size={11} className="shrink-0 text-muted-foreground" />
                <span className="text-foreground truncate">{node.title}</span>
                {node.child_count > 0 && (
                  <span className="text-[10px] text-muted-foreground shrink-0">
                    {node.child_count} sub
                  </span>
                )}
              </div>
            ))}
          </div>

          <div className="flex items-center gap-2 pt-1 border-t border-border">
            <Link2 size={12} className="text-muted-foreground" />
            <code className="text-[10px] text-muted-foreground font-mono break-all">
              cache: {buildResult.cache_key.slice(0, 32)}…
            </code>
          </div>
        </section>
      )}

      {/* Step 3 — Query */}
      {hasTree && (
        <section className="rounded-xl border border-border bg-card p-5 flex flex-col gap-4">
          <div className="flex items-center gap-2">
            <Search size={16} className="text-[var(--v-accent)]" />
            <h3 className="text-sm font-semibold">3 — Ask a question</h3>
            <span className="text-xs text-muted-foreground ml-auto">
              Navigates the section tree by term frequency
            </span>
          </div>

          <div className="flex gap-2">
            <Input
              value={query}
              onChange={(e) => setQuery(e.target.value)}
              placeholder="What does the document say about…"
              className="flex-1"
              onKeyDown={(e) => e.key === "Enter" && handleQuery()}
            />
            <select
              value={k}
              onChange={(e) => setK(Number(e.target.value))}
              className="h-10 rounded-md border border-input bg-background px-2 text-sm text-foreground"
            >
              {[1, 2, 3, 5, 8].map((n) => (
                <option key={n} value={n}>Top {n}</option>
              ))}
            </select>
            <Button
              size="sm"
              onClick={handleQuery}
              disabled={querying || !query.trim()}
              className="gap-2 h-10"
            >
              <Search size={14} />
              {querying ? "Searching…" : "Ask"}
            </Button>
          </div>

          {queryError && (
            <p className="text-sm text-red-500 dark:text-red-400">{queryError}</p>
          )}
        </section>
      )}

      {/* Results */}
      {queryResult && (
        <section className="rounded-xl border border-border bg-card p-5 flex flex-col gap-4">
          <h3 className="text-sm font-semibold">Results</h3>

          {/* Synthesised answer */}
          {queryResult.answer && (
            <div className="rounded-lg bg-[var(--v-accent-muted)] border border-[var(--v-accent-ring)] p-4">
              <p className="text-xs font-medium text-muted-foreground mb-1.5">Answer</p>
              <p className="text-sm text-foreground whitespace-pre-wrap leading-relaxed">{queryResult.answer}</p>
            </div>
          )}

          {/* Citations */}
          {queryResult.citations.length === 0 ? (
            <p className="text-sm text-muted-foreground">No matching sections found.</p>
          ) : (
            <div className="flex flex-col gap-3">
              {queryResult.citations.map((c, i) => (
                <CitationCard key={c.node_id} citation={c} rank={i + 1} />
              ))}
            </div>
          )}

          {/* Reasoning */}
          {queryResult.reasoning && (
            <p className="text-xs text-muted-foreground font-mono bg-muted/40 rounded px-3 py-2">
              {queryResult.reasoning}
            </p>
          )}

          {/* Receipt */}
          <ReceiptPanel receipt={queryResult.receipt} />
        </section>
      )}
    </div>
  );
}

// ── Citation card ─────────────────────────────────────────────────────────────

function CitationCard({ citation, rank }: { citation: Citation; rank: number }) {
  return (
    <div className="rounded-lg border border-border bg-background p-4 flex flex-col gap-2">
      <div className="flex items-start gap-3">
        <span className="text-xs font-mono text-muted-foreground w-5 shrink-0 pt-0.5">#{rank}</span>
        <div className="flex-1 min-w-0">
          <p className="text-sm font-medium text-foreground truncate">{citation.title}</p>
          <p className="text-xs text-muted-foreground mt-0.5 truncate">{citation.breadcrumb}</p>
        </div>
        <span className="text-xs text-muted-foreground shrink-0">
          lines {citation.lines[0]}–{citation.lines[1]}
        </span>
      </div>
    </div>
  );
}

// ── Receipt panel ─────────────────────────────────────────────────────────────

function ReceiptPanel({ receipt }: { receipt: Receipt }) {
  const [show, setShow] = useState(false);

  return (
    <div className="border-t border-border pt-3">
      <button
        onClick={() => setShow((v) => !v)}
        className="flex items-center gap-2 text-xs text-muted-foreground hover:text-foreground transition-colors"
      >
        <Link2 size={12} />
        BLAKE3 receipt chain
        <ChevronRight size={11} className={cn("transition-transform", show && "rotate-90")} />
      </button>

      {show && (
        <div className="mt-3 rounded-lg bg-muted/40 p-3 font-mono text-[10px] text-muted-foreground flex flex-col gap-1">
          <HashRow label="prev" value={receipt.prev_hash} />
          <HashRow label="query" value={receipt.query_hash} />
          <HashRow label="answer" value={receipt.answer_hash} />
          <HashRow label="receipt" value={receipt.receipt_hash} accent />
          <p className="text-[10px] text-muted-foreground pt-1">
            algo={receipt.hash_algo} · ts={new Date(receipt.timestamp * 1000).toISOString()}
          </p>
        </div>
      )}
    </div>
  );
}

function HashRow({ label, value, accent }: { label: string; value: string; accent?: boolean }) {
  return (
    <div className="flex items-center gap-2">
      <span className="w-12 text-right shrink-0 text-muted-foreground">{label}</span>
      <span className={cn("break-all", accent && "text-[var(--v-accent)]")}>{value}</span>
    </div>
  );
}
