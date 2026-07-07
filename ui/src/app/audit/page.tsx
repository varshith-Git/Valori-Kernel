"use client";

import { useEffect, useState } from "react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";

interface ParsedEvent {
  index: number;
  type: "INSERT" | "DELETE" | "SOFT_DELETE" | "NODE" | "EDGE" | "UNKNOWN";
  raw: string;
  recordId: number | null;
  tag: number | null;
}

function parseEvent(line: string): ParsedEvent {
  const idxMatch = line.match(/Event ID (\d+):/);
  const index = idxMatch ? Number(idxMatch[1]) : 0;

  const recMatch = line.match(/Record (\d+)/);
  const recordId = recMatch ? Number(recMatch[1]) : null;
  const tagMatch = line.match(/Tag: (\d+)/);
  const tag = tagMatch ? Number(tagMatch[1]) : null;

  let type: ParsedEvent["type"] = "UNKNOWN";
  if (line.includes("InsertRecord")) type = "INSERT";
  else if (line.includes("SoftDeleteRecord")) type = "SOFT_DELETE";
  else if (line.includes("DeleteRecord")) type = "DELETE";
  else if (line.includes("CreateNode") || line.includes("DeleteNode")) type = "NODE";
  else if (line.includes("CreateEdge") || line.includes("DeleteEdge")) type = "EDGE";

  return { index, type, raw: line, recordId, tag };
}

const TYPE_COLORS: Record<ParsedEvent["type"], string> = {
  INSERT: "bg-emerald-500/15 text-emerald-700 border-emerald-500/30",
  DELETE: "bg-red-500/15 text-red-700 border-red-500/30",
  SOFT_DELETE: "bg-amber-500/15 text-amber-700 border-amber-500/30",
  NODE: "bg-blue-500/15 text-blue-700 border-blue-500/30",
  EDGE: "bg-purple-500/15 text-purple-700 border-purple-500/30",
  UNKNOWN: "bg-card text-muted-foreground border-border",
};

export default function AuditPage() {
  const [events, setEvents] = useState<ParsedEvent[]>([]);
  const [isLoading, setIsLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [filter, setFilter] = useState<ParsedEvent["type"] | "ALL">("ALL");

  const load = async () => {
    setIsLoading(true);
    setError(null);
    try {
      const res = await fetch("/api/timeline");
      if (res.status === 400) {
        setError("event-log-disabled");
        return;
      }
      if (res.status === 503) throw new Error("Node unreachable — is the valori server running?");
      if (!res.ok) throw new Error(`Failed to load audit trail (HTTP ${res.status})`);
      const body = await res.json();
      const lines: string[] = Array.isArray(body) ? body : [];
      setEvents(lines.map(parseEvent).reverse()); // newest first
    } catch (e) {
      setError(e instanceof Error ? e.message : "Failed to load audit trail");
    } finally {
      setIsLoading(false);
    }
  };

  useEffect(() => { load(); }, []);

  const exportCsv = () => {
    const rows = [
      ["#", "Type", "Record ID", "Tag", "Raw"],
      ...filtered.map((e) => [
        e.index,
        e.type,
        e.recordId ?? "",
        e.tag ?? "",
        `"${e.raw.replace(/"/g, '""')}"`,
      ]),
    ];
    const csv = rows.map((r) => r.join(",")).join("\n");
    const blob = new Blob([csv], { type: "text/csv" });
    const url = URL.createObjectURL(blob);
    const a = document.createElement("a");
    a.href = url;
    a.download = `valori-audit-${Date.now()}.csv`;
    a.click();
    URL.revokeObjectURL(url);
  };

  const filtered =
    filter === "ALL" ? events : events.filter((e) => e.type === filter);

  if (error === "event-log-disabled") {
    return (
      <div className="w-full max-w-[1600px]">
        <h1 className="text-xl font-semibold text-foreground">Audit Trail</h1>
        <div className="mt-6 rounded-xl border border-amber-500/30 bg-amber-500/10 p-6">
          <p className="text-sm font-medium text-amber-400">
            Event log not enabled on this node
          </p>
          <p className="mt-2 text-xs text-amber-600">
            Restart Valori with <code className="font-mono bg-amber-500/20 px-1 rounded">VALORI_EVENT_LOG_PATH</code> set:
          </p>
          <pre className="mt-3 rounded bg-background px-4 py-3 text-xs text-accent-foreground font-mono">
{`VALORI_DIM=4 \\
VALORI_CORS_ORIGIN="*" \\
VALORI_EVENT_LOG_PATH=/tmp/valori-events.log \\
cargo run -p valori-node`}
          </pre>
        </div>
      </div>
    );
  }

  return (
    <div className="flex flex-col gap-5 w-full max-w-[1600px]">
      <div className="flex items-center justify-between">
        <div>
          <h1 className="text-xl font-semibold text-foreground">Audit Trail</h1>
          <p className="mt-1 text-sm text-muted-foreground">
            For you — browse every mutation in order, each BLAKE3-chained
          </p>
        </div>
        <div className="flex items-center gap-2">
          <Button
            variant="outline"
            size="sm"
            onClick={load}
            className="border-input text-muted-foreground hover:text-foreground hover:bg-accent"
          >
            Refresh
          </Button>
          <Button
            variant="outline"
            size="sm"
            onClick={exportCsv}
            disabled={filtered.length === 0}
            className="border-input text-muted-foreground hover:text-foreground hover:bg-accent"
          >
            Export CSV
          </Button>
        </div>
      </div>

      {/* Filters */}
      <div className="flex gap-2 flex-wrap">
        {(["ALL", "INSERT", "DELETE", "SOFT_DELETE", "NODE", "EDGE"] as const).map((t) => (
          <button
            key={t}
            onClick={() => setFilter(t)}
            className={`rounded-full px-3 py-1 text-xs transition-colors border ${
              filter === t
                ? "bg-accent text-foreground border-border"
                : "border-border text-muted-foreground hover:border-muted hover:text-accent-foreground"
            }`}
          >
            {t === "ALL" ? `All (${events.length})` : t}
          </button>
        ))}
      </div>

      {isLoading ? (
        <div className="flex flex-col gap-2">
          {[1, 2, 3, 4, 5].map((i) => (
            <div key={i} className="h-12 animate-pulse rounded-lg bg-accent" />
          ))}
        </div>
      ) : error ? (
        <p className="text-sm text-red-400">{error}</p>
      ) : filtered.length === 0 ? (
        <div className="rounded-xl border border-dashed border-border py-12 text-center">
          <p className="text-sm text-muted-foreground">No events yet.</p>
        </div>
      ) : (
        <div className="flex flex-col gap-1.5">
          <div className="grid grid-cols-[3.5rem_7rem_1fr] gap-3 px-3 py-2 text-xs text-muted-foreground uppercase tracking-wider border-b border-border">
            <span>Event #</span><span>Type</span><span>Details</span>
          </div>
          {filtered.map((e) => (
            <div
              key={e.index}
              className="grid grid-cols-[3.5rem_7rem_1fr] gap-3 items-center rounded-lg border border-border bg-card px-3 py-2.5 text-sm hover:border-input transition-colors"
            >
              <span className="font-mono text-xs text-muted-foreground">{e.index}</span>
              <span>
                <span
                  className={`inline-block rounded border px-1.5 py-0.5 text-xs font-medium ${TYPE_COLORS[e.type]}`}
                >
                  {e.type}
                </span>
              </span>
              <span className="text-xs text-muted-foreground font-mono truncate">
                {e.recordId !== null && (
                  <span className="text-card-foreground">Record #{e.recordId} </span>
                )}
                {e.tag !== null && (
                  <span className="text-muted-foreground">tag={e.tag} </span>
                )}
              </span>
            </div>
          ))}
        </div>
      )}
    </div>
  );
}
