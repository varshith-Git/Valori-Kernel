"use client";

import { useState } from "react";
import { Button } from "@/components/ui/button";
import { useEmbeddingConfig } from "@/lib/hooks/useEmbeddingConfig";
import Link from "next/link";

interface CommunitySummary {
  community_id: number;
  member_count: number;
  centroid_record_id: number | null;
}

interface DetectResult {
  community_count: number;
  node_count: number;
  communities: CommunitySummary[];
  receipt: string;
}

interface CommunityHit {
  community_id: number;
  score: number;
  member_count: number;
  sample_node_ids: number[];
}

interface SearchResult {
  communities: CommunityHit[];
  total_communities_searched: number;
}

interface Props {
  namespace: string;
}

export function CommunityTab({ namespace }: Props) {
  const { config: embedCfg } = useEmbeddingConfig();
  const hasEmbed = embedCfg.provider === "ollama" ? !!embedCfg.model : !!embedCfg.apiKey;

  // ── Detect state ─────────────────────────────────────────────────
  const [detectResult, setDetectResult] = useState<DetectResult | null>(null);
  const [detecting, setDetecting] = useState(false);
  const [detectError, setDetectError] = useState<string | null>(null);
  const [maxIter, setMaxIter] = useState(20);
  const [receiptCopied, setReceiptCopied] = useState(false);

  // ── Search state ─────────────────────────────────────────────────
  const [query, setQuery] = useState("");
  const [k, setK] = useState(5);
  const [searchResult, setSearchResult] = useState<SearchResult | null>(null);
  const [searching, setSearching] = useState(false);
  const [searchError, setSearchError] = useState<string | null>(null);

  async function runDetect() {
    setDetecting(true);
    setDetectError(null);
    try {
      const res = await fetch("/api/community?action=detect", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ namespace, max_iter: maxIter }),
      });
      const data = await res.json();
      if (!res.ok) throw new Error(data.error ?? "detect failed");
      setDetectResult(data);
    } catch (e: unknown) {
      setDetectError(e instanceof Error ? e.message : String(e));
    } finally {
      setDetecting(false);
    }
  }

  async function runSearch() {
    if (!query.trim()) return;
    setSearching(true);
    setSearchError(null);
    try {
      // 1. Embed the query text
      const embedRes = await fetch("/api/embed-query", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({
          text: query,
          provider: embedCfg.provider,
          model: embedCfg.model,
          apiKey: embedCfg.apiKey,
          endpoint: embedCfg.endpoint,
        }),
      });
      if (!embedRes.ok) {
        const e = await embedRes.json().catch(() => ({})) as { error?: string };
        throw new Error(e.error ?? "embedding failed");
      }
      const { vector } = await embedRes.json() as { vector: number[] };

      // 2. Search communities with the embedded vector
      const res = await fetch("/api/community?action=search", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ vector, k, namespace }),
      });
      const data = await res.json();
      if (!res.ok) throw new Error(data.error ?? "search failed");
      setSearchResult(data);
    } catch (e: unknown) {
      setSearchError(e instanceof Error ? e.message : String(e));
    } finally {
      setSearching(false);
    }
  }

  function copyReceipt() {
    if (!detectResult) return;
    navigator.clipboard.writeText(detectResult.receipt);
    setReceiptCopied(true);
    setTimeout(() => setReceiptCopied(false), 1500);
  }

  const maxSize = detectResult
    ? Math.max(...detectResult.communities.map((c) => c.member_count), 1)
    : 1;

  return (
    <div className="flex flex-col gap-6">

      {/* ── Step 1: Detect ───────────────────────────────────────────── */}
      <section className="flex flex-col gap-3">
        <div className="flex items-start justify-between gap-4">
          <div>
            <h3 className="text-sm font-medium text-foreground">
              Step 1 — Detect Communities
            </h3>
            <p className="text-xs text-muted-foreground mt-0.5">
              Runs Label Propagation on the knowledge graph — groups connected nodes
              into themed clusters. Produces a BLAKE3 receipt proving the structure.
              No LLM required.
            </p>
          </div>
          <div className="flex items-center gap-3 shrink-0">
            <div className="flex items-center gap-1.5">
              <label className="text-xs text-muted-foreground whitespace-nowrap">max iter</label>
              <input
                type="number"
                min={1}
                max={100}
                value={maxIter}
                onChange={(e) => setMaxIter(Number(e.target.value))}
                className="w-14 rounded border border-input bg-background px-2 py-1 text-xs text-foreground text-center focus:outline-none focus:ring-1 focus:ring-ring"
              />
            </div>
            <Button
              size="sm"
              onClick={runDetect}
              disabled={detecting}
              className="bg-primary text-primary-foreground hover:bg-primary/90 disabled:opacity-40"
            >
              {detecting ? "Detecting…" : "Detect Communities"}
            </Button>
          </div>
        </div>

        {detectError && (
          <p className="text-xs text-red-400 rounded border border-red-900/30 bg-red-950/20 px-3 py-2">
            {detectError}
          </p>
        )}

        {detectResult && (
          <div className="flex flex-col gap-3">
            <div className="grid grid-cols-3 gap-3">
              <StatCard label="Communities" value={String(detectResult.community_count)} />
              <StatCard label="Nodes assigned" value={String(detectResult.node_count)} />
              <StatCard
                label="Avg. size"
                value={
                  detectResult.community_count > 0
                    ? (detectResult.node_count / detectResult.community_count).toFixed(1)
                    : "—"
                }
                sub="nodes / community"
              />
            </div>

            {/* BLAKE3 receipt */}
            <div className="rounded-lg border border-border bg-card px-4 py-3 flex items-center justify-between gap-3">
              <div className="min-w-0">
                <p className="text-[10px] uppercase tracking-widest text-muted-foreground mb-1">
                  BLAKE3 Receipt — tamper-evident proof of community structure
                </p>
                <code className="text-xs font-mono text-muted-foreground truncate block">
                  {detectResult.receipt.slice(0, 48)}…
                </code>
              </div>
              <button
                onClick={copyReceipt}
                className="shrink-0 text-[10px] text-muted-foreground hover:text-foreground border border-border rounded px-2 py-1 transition-colors"
              >
                {receiptCopied ? "✓ copied" : "copy"}
              </button>
            </div>

            {/* Community bars */}
            <div className="flex flex-col gap-1.5">
              <div className="grid grid-cols-[3rem_1fr_5rem] gap-2 px-3 py-1 text-[10px] uppercase tracking-widest text-muted-foreground border-b border-border">
                <span>ID</span><span>Size</span><span className="text-right">Nodes</span>
              </div>
              <div className="max-h-52 overflow-y-auto flex flex-col gap-1">
                {detectResult.communities
                  .slice()
                  .sort((a, b) => b.member_count - a.member_count)
                  .map((c) => (
                    <div
                      key={c.community_id}
                      className="grid grid-cols-[3rem_1fr_5rem] gap-2 items-center rounded-lg border border-border bg-card px-3 py-2"
                    >
                      <span className="font-mono text-xs text-muted-foreground">
                        #{c.community_id}
                      </span>
                      <div className="h-2 rounded-full bg-muted overflow-hidden">
                        <div
                          className="h-full rounded-full bg-[var(--v-accent)]"
                          style={{ width: `${Math.min(100, (c.member_count / maxSize) * 100)}%` }}
                        />
                      </div>
                      <span className="font-mono text-xs text-muted-foreground text-right">
                        {c.member_count}
                      </span>
                    </div>
                  ))}
              </div>
            </div>
          </div>
        )}
      </section>

      {/* ── Step 2: Search ───────────────────────────────────────────── */}
      <section className="flex flex-col gap-3 border-t border-border pt-6">
        <div>
          <h3 className="text-sm font-medium text-foreground">
            Step 2 — Search by Theme
          </h3>
          <p className="text-xs text-muted-foreground mt-0.5">
            Type a question or topic — it is embedded and matched against community
            centroids to find the closest themes. Run Step 1 first.
          </p>
        </div>

        {!hasEmbed && (
          <p className="text-xs text-amber-500 rounded border border-amber-800/30 bg-amber-950/20 px-3 py-2">
            No embedding model configured.{" "}
            <Link href="/settings" className="underline">Configure in Settings →</Link>
          </p>
        )}

        <div className="flex flex-col gap-2">
          <div className="flex items-center justify-between">
            <label className="text-xs text-muted-foreground">
              What topic or question do you want to find themes for?
            </label>
            <div className="flex items-center gap-1.5">
              <label className="text-xs text-muted-foreground">k =</label>
              <input
                type="number"
                min={1}
                max={50}
                value={k}
                onChange={(e) => setK(Number(e.target.value))}
                className="w-14 rounded border border-input bg-background px-2 py-1 text-xs text-foreground text-center focus:outline-none focus:ring-1 focus:ring-ring"
              />
            </div>
          </div>
          <input
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            onKeyDown={(e) => e.key === "Enter" && runSearch()}
            placeholder="e.g. machine learning applications in healthcare"
            disabled={!hasEmbed}
            className="w-full rounded-lg border border-input bg-background px-3 py-2.5 text-sm text-foreground placeholder:text-muted-foreground focus:outline-none focus:ring-1 focus:ring-ring disabled:opacity-50"
          />
          <div className="flex items-center gap-2">
            <Button
              size="sm"
              onClick={runSearch}
              disabled={searching || !query.trim() || !hasEmbed}
              className="bg-primary text-primary-foreground hover:bg-primary/90 disabled:opacity-40"
            >
              {searching ? "Searching…" : "Find Themes →"}
            </Button>
            <span className="text-xs text-muted-foreground">↵ Enter to run</span>
          </div>
        </div>

        {searchError && (
          <p className="text-xs text-red-400 rounded border border-red-900/30 bg-red-950/20 px-3 py-2">
            {searchError}
          </p>
        )}

        {searchResult && (
          <div className="flex flex-col gap-2">
            <p className="text-xs text-muted-foreground">
              Top {searchResult.communities.length} of{" "}
              {searchResult.total_communities_searched} communities
            </p>
            {searchResult.communities.map((hit, i) => (
              <div
                key={hit.community_id}
                className="rounded-lg border border-border bg-card px-4 py-3 flex flex-col gap-2"
              >
                <div className="flex items-center justify-between">
                  <div className="flex items-center gap-2">
                    <span className="text-xs text-muted-foreground font-mono">#{i + 1}</span>
                    <span className="text-sm font-medium text-foreground">
                      Community {hit.community_id}
                    </span>
                    <span className="text-xs text-muted-foreground">
                      {hit.member_count} nodes
                    </span>
                  </div>
                  <span
                    className={`font-mono text-xs px-2 py-0.5 rounded border border-border bg-muted ${
                      hit.score > 0.7
                        ? "text-emerald-400"
                        : hit.score > 0.4
                        ? "text-amber-400"
                        : "text-muted-foreground"
                    }`}
                  >
                    {(hit.score * 100).toFixed(1)}% match
                  </span>
                </div>
                {hit.sample_node_ids.length > 0 && (
                  <div className="flex flex-wrap gap-1">
                    {hit.sample_node_ids.map((nid) => (
                      <span
                        key={nid}
                        className="text-[10px] font-mono px-1.5 py-0.5 rounded bg-muted text-muted-foreground border border-border"
                      >
                        node {nid}
                      </span>
                    ))}
                    {hit.member_count > hit.sample_node_ids.length && (
                      <span className="text-[10px] text-muted-foreground px-1">
                        +{hit.member_count - hit.sample_node_ids.length} more
                      </span>
                    )}
                  </div>
                )}
              </div>
            ))}
          </div>
        )}
      </section>
    </div>
  );
}

function StatCard({ label, value, sub }: { label: string; value: string; sub?: string }) {
  return (
    <div className="rounded-xl border border-border bg-card px-4 py-3">
      <p className="text-[10px] uppercase tracking-widest text-muted-foreground">{label}</p>
      <p className="mt-1 font-mono text-xl font-semibold text-foreground">{value}</p>
      {sub && <p className="mt-0.5 text-xs text-muted-foreground">{sub}</p>}
    </div>
  );
}
