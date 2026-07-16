"use client";

import { useState, useRef, useEffect } from "react";
import { Button } from "@/components/ui/button";
import { useLLMConfig } from "@/lib/hooks/useLLMConfig";

interface ExtractedEntity {
  name: string;
  type: string;
  description: string;
  node_id: number;
  record_id: number | null;
}

interface InsertedRelationship {
  source_name: string;
  target_name: string;
  description: string;
  edge_id: number;
}

interface ExtractionResult {
  entities: ExtractedEntity[];
  relationships: InsertedRelationship[];
  entity_count: number;
  relationship_count: number;
  skipped_relationships: number;
}

const KIND_COLORS: Record<string, string> = {
  PERSON:       "bg-blue-500/15 text-blue-400 border-blue-800/40",
  ORGANIZATION: "bg-purple-500/15 text-purple-400 border-purple-800/40",
  CONCEPT:      "bg-emerald-500/15 text-emerald-400 border-emerald-800/40",
  LOCATION:     "bg-amber-500/15 text-amber-400 border-amber-800/40",
  EVENT:        "bg-rose-500/15 text-rose-400 border-rose-800/40",
};

function kindColor(k: string) {
  return KIND_COLORS[k.toUpperCase()] ?? "bg-muted text-muted-foreground border-border";
}

interface Props {
  namespace: string;
}

export function EntityExtractionTab({ namespace }: Props) {
  const { config: llmCfg } = useLLMConfig();
  const [text, setText] = useState("");
  const [model, setModel] = useState("");
  const [running, setRunning] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [result, setResult] = useState<ExtractionResult | null>(null);
  const [view, setView] = useState<"graph" | "list">("graph");

  const canvasRef = useRef<HTMLCanvasElement>(null);

  async function extract() {
    if (!text.trim()) return;
    setRunning(true);
    setError(null);
    setResult(null);
    try {
      const body: Record<string, unknown> = {
        text,
        namespace,
        provider: llmCfg.provider,
        model: model.trim() || llmCfg.model,
        url: llmCfg.endpoint,
        api_key: llmCfg.apiKey,
      };
      const res = await fetch("/api/extract-entities", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify(body),
      });
      const data = await res.json();
      if (!res.ok) throw new Error(data.error ?? "extraction failed");
      setResult(data);
    } catch (e: unknown) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setRunning(false);
    }
  }

  // Simple force-directed graph drawn on canvas
  useEffect(() => {
    if (!result || view !== "graph") return;
    const canvas = canvasRef.current;
    if (!canvas) return;

    const entities = result.entities;
    const rels = result.relationships;
    if (entities.length === 0) return;

    const W = canvas.width = canvas.offsetWidth * window.devicePixelRatio;
    const H = canvas.height = 340 * window.devicePixelRatio;
    const scale = window.devicePixelRatio;
    const ctx = canvas.getContext("2d")!;
    ctx.scale(scale, scale);
    const w = W / scale;
    const h = H / scale;

    // Place nodes in a circle initially
    const nodes = entities.map((e, i) => {
      const angle = (2 * Math.PI * i) / entities.length - Math.PI / 2;
      const r = Math.min(w, h) * 0.32;
      return {
        id: e.name,
        x: w / 2 + r * Math.cos(angle),
        y: h / 2 + r * Math.sin(angle),
        vx: 0,
        vy: 0,
        kind: e.type,
      };
    });

    const nodeMap = new Map(nodes.map((n) => [n.id, n]));

    // Simple force sim — 30 iterations
    for (let iter = 0; iter < 80; iter++) {
      // Repulsion
      for (let a = 0; a < nodes.length; a++) {
        for (let b = a + 1; b < nodes.length; b++) {
          const na = nodes[a], nb = nodes[b];
          const dx = na.x - nb.x;
          const dy = na.y - nb.y;
          const dist = Math.sqrt(dx * dx + dy * dy) || 1;
          const force = 2400 / (dist * dist);
          const fx = (dx / dist) * force;
          const fy = (dy / dist) * force;
          na.vx += fx; na.vy += fy;
          nb.vx -= fx; nb.vy -= fy;
        }
      }
      // Attraction along edges
      for (const rel of rels) {
        const a = nodeMap.get(rel.source_name);
        const b = nodeMap.get(rel.target_name);
        if (!a || !b) continue;
        const dx = b.x - a.x;
        const dy = b.y - a.y;
        const dist = Math.sqrt(dx * dx + dy * dy) || 1;
        const force = (dist - 120) * 0.03;
        const fx = (dx / dist) * force;
        const fy = (dy / dist) * force;
        a.vx += fx; a.vy += fy;
        b.vx -= fx; b.vy -= fy;
      }
      // Center gravity
      for (const n of nodes) {
        n.vx += (w / 2 - n.x) * 0.012;
        n.vy += (h / 2 - n.y) * 0.012;
      }
      // Dampen & apply
      for (const n of nodes) {
        n.vx *= 0.82; n.vy *= 0.82;
        n.x += n.vx; n.y += n.vy;
        n.x = Math.max(48, Math.min(w - 48, n.x));
        n.y = Math.max(28, Math.min(h - 28, n.y));
      }
    }

    // Detect dark mode
    const isDark = document.documentElement.classList.contains("dark");
    const edgeColor = isDark ? "rgba(255,255,255,0.12)" : "rgba(0,0,0,0.10)";
    const labelColor = isDark ? "rgba(255,255,255,0.75)" : "rgba(0,0,0,0.75)";
    const bgCard = isDark ? "#1a1a2e" : "#f8f8fc";

    ctx.clearRect(0, 0, w, h);

    // Draw edges
    ctx.strokeStyle = edgeColor;
    ctx.lineWidth = 1.5;
    for (const rel of rels) {
      const a = nodeMap.get(rel.source_name);
      const b = nodeMap.get(rel.target_name);
      if (!a || !b) continue;
      ctx.beginPath();
      ctx.moveTo(a.x, a.y);
      ctx.lineTo(b.x, b.y);
      ctx.stroke();
    }

    // Node color map
    const COLORS: Record<string, string> = {
      PERSON: "#60a5fa",
      ORGANIZATION: "#a78bfa",
      CONCEPT: "#34d399",
      LOCATION: "#fbbf24",
      EVENT: "#f87171",
    };

    // Draw nodes
    for (const n of nodes) {
      const color = COLORS[n.kind?.toUpperCase()] ?? "#94a3b8";
      ctx.beginPath();
      ctx.arc(n.x, n.y, 10, 0, Math.PI * 2);
      ctx.fillStyle = color + "33";
      ctx.fill();
      ctx.strokeStyle = color;
      ctx.lineWidth = 2;
      ctx.stroke();

      // Label
      ctx.fillStyle = labelColor;
      ctx.font = `${11}px system-ui, sans-serif`;
      ctx.textAlign = "center";
      ctx.fillText(n.id.length > 14 ? n.id.slice(0, 13) + "…" : n.id, n.x, n.y + 22);
    }
  }, [result, view]);

  return (
    <div className="flex flex-col gap-5">
      <div>
        <h3 className="text-sm font-medium text-foreground">Entity Extraction</h3>
        <p className="text-xs text-muted-foreground mt-0.5">
          Paste text → the LLM extracts entities and relationships → they are inserted
          as Concept nodes + Relation edges into the knowledge graph. Requires{" "}
          <code className="text-[10px] bg-muted px-1 rounded">VALORI_EMBED_PROVIDER</code>.
        </p>
      </div>

      {/* Input */}
      <div className="flex flex-col gap-2">
        <textarea
          value={text}
          onChange={(e) => setText(e.target.value)}
          placeholder="Paste a paragraph or document excerpt here…&#10;&#10;e.g. 'OpenAI was founded by Sam Altman and Elon Musk in 2015. The company developed GPT-4 and partnered with Microsoft.'"
          rows={5}
          className="w-full rounded-lg border border-input bg-background px-3 py-2 text-sm text-foreground placeholder:text-muted-foreground focus:outline-none focus:ring-1 focus:ring-ring resize-none"
        />
        <div className="flex items-center gap-3">
          <Button
            size="sm"
            onClick={extract}
            disabled={running || !text.trim()}
            className="bg-primary text-primary-foreground hover:bg-primary/90 disabled:opacity-40"
          >
            {running ? "Extracting…" : "Extract Entities →"}
          </Button>
          <input
            value={model}
            onChange={(e) => setModel(e.target.value)}
            placeholder="model override (optional)"
            className="flex-1 rounded border border-input bg-background px-3 py-1.5 text-xs text-foreground placeholder:text-muted-foreground focus:outline-none focus:ring-1 focus:ring-ring"
          />
        </div>
      </div>

      {error && (
        <p className="text-xs text-red-400 rounded border border-red-900/30 bg-red-950/20 px-3 py-2">
          {error}
        </p>
      )}

      {result && (
        <div className="flex flex-col gap-4">
          {/* Stats */}
          <div className="grid grid-cols-3 gap-3">
            <StatCard label="Entities" value={String(result.entity_count)} />
            <StatCard label="Relationships" value={String(result.relationship_count)} />
            <StatCard
              label="Skipped"
              value={String(result.skipped_relationships)}
              sub="unknown endpoints"
              warn={result.skipped_relationships > 0}
            />
          </div>

          {/* View toggle */}
          <div className="flex items-center gap-2">
            <button
              onClick={() => setView("graph")}
              className={`text-xs px-3 py-1 rounded border transition-colors ${
                view === "graph"
                  ? "bg-primary text-primary-foreground border-primary"
                  : "border-border text-muted-foreground hover:text-foreground"
              }`}
            >
              Graph view
            </button>
            <button
              onClick={() => setView("list")}
              className={`text-xs px-3 py-1 rounded border transition-colors ${
                view === "list"
                  ? "bg-primary text-primary-foreground border-primary"
                  : "border-border text-muted-foreground hover:text-foreground"
              }`}
            >
              List view
            </button>
          </div>

          {view === "graph" ? (
            <div className="rounded-xl border border-border bg-card overflow-hidden">
              <canvas
                ref={canvasRef}
                style={{ width: "100%", height: 340, display: "block" }}
              />
              {/* Legend */}
              <div className="flex flex-wrap gap-3 px-4 py-3 border-t border-border">
                {Object.entries({ PERSON: "#60a5fa", ORGANIZATION: "#a78bfa", CONCEPT: "#34d399", LOCATION: "#fbbf24", EVENT: "#f87171" }).map(([k, c]) => (
                  <div key={k} className="flex items-center gap-1.5">
                    <span className="w-2.5 h-2.5 rounded-full" style={{ background: c }} />
                    <span className="text-[10px] text-muted-foreground">{k}</span>
                  </div>
                ))}
              </div>
            </div>
          ) : (
            <div className="flex flex-col gap-4">
              {/* Entities list */}
              <div className="flex flex-col gap-2">
                <h4 className="text-xs font-medium text-muted-foreground uppercase tracking-widest">
                  Entities ({result.entity_count})
                </h4>
                <div className="flex flex-col gap-1.5 max-h-64 overflow-y-auto">
                  {result.entities.map((e) => (
                    <div
                      key={e.node_id}
                      className="rounded-lg border border-border bg-card px-3 py-2.5 flex items-start gap-3"
                    >
                      <span className={`text-[10px] font-mono px-1.5 py-0.5 rounded border shrink-0 mt-0.5 ${kindColor(e.type)}`}>
                        {e.type}
                      </span>
                      <div className="min-w-0">
                        <p className="text-sm font-medium text-foreground">{e.name}</p>
                        <p className="text-xs text-muted-foreground mt-0.5 line-clamp-2">{e.description}</p>
                        <p className="text-[10px] font-mono text-muted-foreground mt-1">
                          node #{e.node_id}{e.record_id != null ? ` · rec #${e.record_id}` : ""}
                        </p>
                      </div>
                    </div>
                  ))}
                </div>
              </div>

              {/* Relationships list */}
              {result.relationships.length > 0 && (
                <div className="flex flex-col gap-2">
                  <h4 className="text-xs font-medium text-muted-foreground uppercase tracking-widest">
                    Relationships ({result.relationship_count})
                  </h4>
                  <div className="flex flex-col gap-1.5 max-h-48 overflow-y-auto">
                    {result.relationships.map((r) => (
                      <div
                        key={r.edge_id}
                        className="rounded-lg border border-border bg-card px-3 py-2 flex items-center gap-2 text-xs"
                      >
                        <span className="font-medium text-foreground shrink-0">{r.source_name}</span>
                        <span className="text-muted-foreground">→</span>
                        <span className="text-muted-foreground flex-1 min-w-0 truncate">{r.description}</span>
                        <span className="text-muted-foreground">→</span>
                        <span className="font-medium text-foreground shrink-0">{r.target_name}</span>
                        <span className="text-[10px] font-mono text-muted-foreground shrink-0 ml-2">
                          e#{r.edge_id}
                        </span>
                      </div>
                    ))}
                  </div>
                </div>
              )}
            </div>
          )}
        </div>
      )}
    </div>
  );
}

function StatCard({ label, value, sub, warn }: { label: string; value: string; sub?: string; warn?: boolean }) {
  return (
    <div className="rounded-xl border border-border bg-card px-4 py-3">
      <p className="text-[10px] uppercase tracking-widest text-muted-foreground">{label}</p>
      <p className={`mt-1 font-mono text-xl font-semibold ${warn ? "text-amber-400" : "text-foreground"}`}>{value}</p>
      {sub && <p className={`mt-0.5 text-xs ${warn ? "text-amber-600" : "text-muted-foreground"}`}>{sub}</p>}
    </div>
  );
}
