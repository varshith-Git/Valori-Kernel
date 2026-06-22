"use client";

import { useState, useCallback } from "react";
import useSWR from "swr";
import type { NsAuditResponse, NsEvent } from "@/app/api/namespace-audit/route";

const fetcher = (url: string) => fetch(url).then((r) => r.json());

// -- Color per event kind -----------------------------------------------------
function kindColor(kind: string): string {
  if (/InsertRecord|AutoInsert/.test(kind))       return "#4ade80";
  if (/DeleteRecord|SoftDelete/.test(kind))        return "#f87171";
  if (/CreateNode|AutoCreateNode/.test(kind))      return "#38bdf8";
  if (/DeleteNode/.test(kind))                     return "#fb923c";
  if (/CreateEdge|AutoCreateEdge/.test(kind))      return "#a78bfa";
  if (/DeleteEdge/.test(kind))                     return "#fbbf24";
  return "#71717a";
}

// -- Copy button --------------------------------------------------------------
function CopyBtn({ text, label = "copy" }: { text: string; label?: string }) {
  const [done, setDone] = useState(false);
  const copy = useCallback(async () => {
    await navigator.clipboard.writeText(text);
    setDone(true);
    setTimeout(() => setDone(false), 1500);
  }, [text]);
  return (
    <button
      onClick={copy}
      className={`text-[10px] px-2 py-0.5 rounded border transition-all flex-shrink-0 ${
        done
          ? "border-emerald-700 bg-emerald-950/40 text-emerald-400"
          : "border-input bg-card text-muted-foreground hover:text-card-foreground hover:border-ring"
      }`}
    >
      {done ? "✓" : label}
    </button>
  );
}

// -- Hash display -------------------------------------------------------------
function HashRow({ label, hash, note }: { label: string; hash: string; note?: string }) {
  return (
    <div className="flex flex-col gap-1 py-3 border-b border-border last:border-0">
      <div className="flex items-center gap-2 justify-between">
        <span className="text-xs text-muted-foreground">{label}</span>
        <CopyBtn text={hash} />
      </div>
      <code className="font-mono text-[11px] text-emerald-400 break-all leading-relaxed">
        {hash}
      </code>
      {note && <p className="text-[10px] text-muted-foreground">{note}</p>}
    </div>
  );
}

// -- Stat pill ----------------------------------------------------------------
function Stat({ label, value, sub }: { label: string; value: string | number; sub?: string }) {
  return (
    <div className="flex flex-col gap-0.5 rounded-lg bg-accent/50 border border-border px-4 py-3">
      <p className="text-[10px] text-muted-foreground uppercase tracking-widest">{label}</p>
      <p className="text-xl font-semibold text-foreground tabular-nums">{value}</p>
      {sub && <p className="text-[10px] text-zinc-700">{sub}</p>}
    </div>
  );
}

// -- Single event line --------------------------------------------------------
function EventLine({ ev, idx }: { ev: NsEvent; idx: number }) {
  const [hover, setHover] = useState(false);
  return (
    <div
      onMouseEnter={() => setHover(true)}
      onMouseLeave={() => setHover(false)}
      className="flex items-start gap-3 px-4 py-[3px] hover:bg-white/[0.025] font-mono text-[12px] leading-5"
    >
      <span className="flex-shrink-0 w-8 text-right text-zinc-700 select-none tabular-nums text-[10px] mt-0.5">
        {idx + 1}
      </span>
      <span
        className="flex-1 min-w-0 break-all whitespace-pre-wrap"
        style={{ color: kindColor(ev.kind) }}
      >
        {ev.raw}
      </span>
      {hover && <CopyBtn text={ev.raw} />}
    </div>
  );
}

// -- Main tab -----------------------------------------------------------------
export function VerifyTab({ namespace }: { namespace: string }) {
  const [filter, setFilter] = useState("");
  const [activeKinds, setActiveKinds] = useState<Set<string>>(new Set());
  const [showIds, setShowIds] = useState(false);

  const { data, isLoading, error, mutate } = useSWR<NsAuditResponse>(
    `/api/namespace-audit?namespace=${encodeURIComponent(namespace)}`,
    fetcher,
    { revalidateOnFocus: false }
  );

  if (isLoading) {
    return (
      <div className="flex items-center gap-2.5 py-10 text-xs text-muted-foreground">
        <span className="h-3 w-3 animate-spin rounded-full border-2 border-muted border-t-zinc-300" />
        Computing namespace audit…
      </div>
    );
  }

  if (error || !data) {
    return (
      <p className="text-sm text-red-500 py-6">
        Failed to load audit data. Is the event log enabled?
      </p>
    );
  }

  const allKinds = [...new Set(data.events.map((e) => e.kind))].sort();
  const filterLow = filter.toLowerCase();
  const filtered = data.events.filter((e) => {
    if (filterLow && !e.raw.toLowerCase().includes(filterLow)) return false;
    if (activeKinds.size > 0 && !activeKinds.has(e.kind)) return false;
    return true;
  });

  const toggleKind = (k: string) =>
    setActiveKinds((prev) => {
      const next = new Set(prev);
      if (next.has(k)) next.delete(k); else next.add(k);
      return next;
    });

  const coverage = data.total_events > 0
    ? ((data.events.length / data.total_events) * 100).toFixed(1)
    : "0";

  const insertCount  = data.events.filter((e) => e.kind.includes("Insert")).length;
  const deleteCount  = data.events.filter((e) => e.kind.includes("Delete")).length;
  const nodeCount    = data.events.filter((e) => e.kind.includes("Node")).length;
  const edgeCount    = data.events.filter((e) => e.kind.includes("Edge")).length;

  return (
    <div className="flex flex-col gap-6">

      {/* -- Proof panel -- */}
      <div className="rounded-xl border border-border bg-card overflow-hidden">
        <div className="flex items-center justify-between px-5 py-3 border-b border-border bg-background/50">
          <h2 className="text-xs font-semibold text-accent-foreground uppercase tracking-widest">
            Verifiable proof
          </h2>
          <div className="flex items-center gap-2">
            <span className="text-[10px] text-emerald-500">
              ● {data.events.length} events · {coverage}% of global log
            </span>
            <button
              onClick={() => mutate()}
              className="text-[10px] text-muted-foreground hover:text-muted-foreground transition-colors"
            >
              ↻ refresh
            </button>
          </div>
        </div>

        {/* Stats row */}
        <div className="grid grid-cols-2 sm:grid-cols-4 gap-3 px-5 py-4 border-b border-border">
          <Stat label="Records" value={data.record_count} />
          <Stat label="Nodes" value={data.node_count} />
          <Stat label="Inserts" value={insertCount} />
          <Stat label="Deletes" value={deleteCount} />
        </div>

        {/* Hashes */}
        <div className="px-5 py-1">
          <HashRow
            label="Namespace proof (SHA-256 of event IDs)"
            hash={data.ns_proof_hash}
            note={`Computed from ${data.ns_event_ids.length} event IDs belonging to this collection. Reproducible from the same event log.`}
          />
          {data.global_state_hash && (
            <HashRow
              label="Global kernel state (BLAKE3)"
              hash={data.global_state_hash}
              note="BLAKE3 Merkle root over ALL applied events across all namespaces."
            />
          )}
          {data.global_event_log_hash && (
            <HashRow
              label="Global event log file (BLAKE3)"
              hash={data.global_event_log_hash}
              note="BLAKE3 hash of the on-disk events.log file."
            />
          )}
          {data.global_event_count !== null && (
            <div className="py-3">
              <div className="flex items-center justify-between text-[11px]">
                <span className="text-muted-foreground">
                  This collection owns {data.events.length} of {data.global_event_count} total events ({coverage}%)
                </span>
                <button
                  onClick={() => setShowIds((v) => !v)}
                  className="text-zinc-700 hover:text-muted-foreground transition-colors"
                >
                  {showIds ? "hide event IDs ▲" : "show event IDs ▼"}
                </button>
              </div>
              {showIds && (
                <div className="mt-2 flex items-start gap-2">
                  <code className="flex-1 font-mono text-[10px] text-muted-foreground break-all leading-relaxed bg-background rounded-lg border border-border px-3 py-2">
                    {data.ns_event_ids.join(", ")}
                  </code>
                  <CopyBtn text={data.ns_event_ids.join(",")} label="copy IDs" />
                </div>
              )}
            </div>
          )}
        </div>

        {/* How to verify callout */}
        <details className="border-t border-border">
          <summary className="px-5 py-3 text-[11px] text-muted-foreground cursor-pointer hover:text-muted-foreground transition-colors">
            How to verify this proof independently
          </summary>
          <div className="px-5 pb-4 flex flex-col gap-2 text-[11px] text-muted-foreground leading-relaxed">
            <p>1. Run <code className="text-muted-foreground">valori-verify --event-log /path/to/events.log</code> to replay the chain and confirm the global BLAKE3 hashes match.</p>
            <p>2. List all records/nodes for this collection (namespace <code className="text-muted-foreground">{namespace}</code>) using the API or CLI.</p>
            <p>3. Filter timeline events that reference those record/node IDs.</p>
            <p>4. Sort those event IDs numerically, join with commas, compute SHA-256. It should equal the namespace proof hash above.</p>
          </div>
        </details>
      </div>

      {/* -- Audit trail -- */}
      <div className="rounded-xl border border-border overflow-hidden bg-background">
        {/* Header */}
        <div className="flex items-center gap-3 px-4 py-2.5 border-b border-border bg-background flex-wrap">
          <span className="font-mono text-xs text-muted-foreground">
            audit trail
            <span className="text-zinc-700"> · {data.namespace}</span>
          </span>
          <span className="text-[10px] text-muted-foreground tabular-nums">
            {filtered.length}
            {filtered.length !== data.events.length && (
              <span className="text-zinc-700"> / {data.events.length}</span>
            )} events
          </span>
          <div className="flex-1" />
          <div className="flex items-center gap-2">
            <span className="text-[10px] text-muted-foreground">
              {nodeCount} node · {edgeCount} edge
            </span>
            <CopyBtn text={filtered.map((e) => e.raw).join("\n")} label="copy all" />
          </div>
        </div>

        {/* Filter + kind chips */}
        <div className="flex items-center gap-2 px-4 py-2 border-b border-border/60 flex-wrap bg-background/80">
          <div className="relative">
            <span className="absolute left-2.5 top-1/2 -translate-y-1/2 text-muted-foreground font-mono text-[11px]">/</span>
            <input
              type="text"
              value={filter}
              onChange={(e) => setFilter(e.target.value)}
              placeholder="filter…"
              className="font-mono text-[12px] bg-card border border-input rounded px-3 py-1 pl-6 w-44 text-card-foreground placeholder-zinc-700 outline-none focus:border-zinc-500 transition-colors"
            />
            {filter && (
              <button onClick={() => setFilter("")} className="absolute right-2 top-1/2 -translate-y-1/2 text-muted-foreground text-xs">×</button>
            )}
          </div>
          {allKinds.map((k) => (
            <button
              key={k}
              onClick={() => toggleKind(k)}
              style={activeKinds.has(k) || activeKinds.size === 0 ? { color: kindColor(k) } : undefined}
              className={`font-mono text-[10px] px-1.5 py-0.5 rounded border transition-all ${
                activeKinds.size > 0 && !activeKinds.has(k)
                  ? "border-border text-zinc-700 opacity-40"
                  : "border-input hover:border-ring"
              }`}
            >
              {k}
            </button>
          ))}
          {activeKinds.size > 0 && (
            <button onClick={() => setActiveKinds(new Set())} className="text-[10px] text-zinc-700 hover:text-muted-foreground">
              clear
            </button>
          )}
        </div>

        {/* Event list */}
        <div className="max-h-[60vh] overflow-y-auto">
          {filtered.length === 0 ? (
            <p className="px-4 py-8 text-xs text-muted-foreground text-center">
              {data.events.length === 0
                ? "No events found for this collection. Event log may not be enabled."
                : "No events match the current filter."}
            </p>
          ) : (
            filtered.map((ev, i) => <EventLine key={ev.event_id} ev={ev} idx={i} />)
          )}
        </div>
      </div>
    </div>
  );
}
