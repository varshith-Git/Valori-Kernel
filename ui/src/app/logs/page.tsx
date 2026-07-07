"use client";

import { useState, useEffect, useRef, useCallback } from "react";
import useSWR from "swr";

// -- Colour-code each event kind -----------------------------------------------
function lineColor(line: string): string {
  if (/InsertRecord|AutoInsertRecord/.test(line))      return "#4ade80"; // emerald
  if (/DeleteRecord|SoftDeleteRecord/.test(line))      return "#f87171"; // red
  if (/CreateNode|AutoCreateNode/.test(line))          return "#38bdf8"; // sky
  if (/DeleteNode/.test(line))                         return "#fb923c"; // orange
  if (/CreateEdge|AutoCreateEdge/.test(line))          return "#a78bfa"; // violet
  if (/DeleteEdge/.test(line))                         return "#fbbf24"; // amber
  if (/ShredKey|InsertRecordEncrypted/.test(line))     return "#f472b6"; // pink
  return "#71717a";                                                       // zinc-500
}

// -- Extract event kind label --------------------------------------------------
function eventKind(line: string): string {
  const m = line.match(/:\s+([A-Za-z]+(?:[A-Z][a-z]+)*)/);
  return m ? m[1] : "Event";
}

// -- Copy button ---------------------------------------------------------------
function CopyBtn({
  text,
  label = "copy",
  className = "",
}: {
  text: string;
  label?: string;
  className?: string;
}) {
  const [done, setDone] = useState(false);
  const copy = async () => {
    try {
      await navigator.clipboard.writeText(text);
      setDone(true);
      setTimeout(() => setDone(false), 1600);
    } catch { /* clipboard denied — no-op */ }
  };
  return (
    <button
      onClick={copy}
      className={`text-[10px] px-2 py-0.5 rounded border transition-all select-none ${
        done
          ? "border-emerald-300 bg-emerald-100 text-emerald-700 dark:border-emerald-700 dark:bg-emerald-950/50 dark:text-emerald-400"
          : "border-input bg-card text-muted-foreground hover:text-foreground hover:border-ring"
      } ${className}`}
    >
      {done ? "✓" : label}
    </button>
  );
}

// -- Single log line -----------------------------------------------------------
function LogLine({ index, line }: { index: number; line: string }) {
  const [hover, setHover] = useState(false);

  return (
    <div
      onMouseEnter={() => setHover(true)}
      onMouseLeave={() => setHover(false)}
      className="group flex items-start gap-3 px-4 py-[3px] hover:bg-white/[0.03] font-mono text-[12.5px] leading-5"
    >
      {/* Line number */}
      <span className="flex-shrink-0 w-10 text-right text-muted-foreground select-none tabular-nums">
        {index + 1}
      </span>

      {/* Content */}
      <span className="flex-1 min-w-0 break-all whitespace-pre-wrap" style={{ color: lineColor(line) }}>
        {line}
      </span>

      {/* Copy on hover */}
      {hover && (
        <span className="flex-shrink-0">
          <CopyBtn text={line} />
        </span>
      )}
    </div>
  );
}

// -- Main page -----------------------------------------------------------------
const fetcher = (url: string) =>
  fetch(url).then((r) => {
    if (!r.ok) throw new Error(`HTTP ${r.status}`);
    return r.json();
  });

export default function LogsPage() {
  const [filter, setFilter] = useState("");
  const [autoScroll, setAutoScroll] = useState(true);
  const [liveRefresh, setLiveRefresh] = useState(true);
  const [kinds, setKinds] = useState<Set<string>>(new Set()); // empty = show all
  const bottomRef = useRef<HTMLDivElement>(null);
  const containerRef = useRef<HTMLDivElement>(null);

  const { data, error, isLoading } = useSWR<string[]>(
    "/api/timeline",
    fetcher,
    { refreshInterval: liveRefresh ? 2000 : 0, revalidateOnFocus: false }
  );

  const rawLines: string[] = Array.isArray(data) ? data : [];
  const errorStatus = error instanceof Error ? Number(error.message.replace("HTTP ", "")) : undefined;
  const notEnabled = errorStatus === 400;

  // Filter
  const filterLower = filter.toLowerCase();
  const lines = rawLines.filter((l) => {
    if (filterLower && !l.toLowerCase().includes(filterLower)) return false;
    if (kinds.size > 0 && !kinds.has(eventKind(l))) return false;
    return true;
  });

  // Auto-scroll to bottom when new lines arrive
  useEffect(() => {
    if (autoScroll && bottomRef.current) {
      bottomRef.current.scrollIntoView({ behavior: "smooth" });
    }
  }, [lines.length, autoScroll]);

  // Pause auto-scroll when user scrolls up
  const handleScroll = useCallback(() => {
    const el = containerRef.current;
    if (!el) return;
    const atBottom = el.scrollHeight - el.scrollTop - el.clientHeight < 60;
    setAutoScroll(atBottom);
  }, []);

  // Build available kind list from raw data
  const allKinds = [...new Set(rawLines.map(eventKind))].sort();

  const toggleKind = (k: string) => {
    setKinds((prev) => {
      const next = new Set(prev);
      if (next.has(k)) next.delete(k); else next.add(k);
      return next;
    });
  };

  const allText = lines.join("\n");

  return (
    <div className="flex flex-col bg-background text-accent-foreground overflow-hidden -mx-6 -my-6 h-[calc(100vh-3rem)]">

      {/* -- Top bar -- */}
      <div className="flex-shrink-0 flex items-center gap-3 px-4 py-2.5 border-b border-border bg-background">
        {/* Title */}
        <span className="font-mono text-sm font-semibold text-foreground">
          valori<span className="text-muted-foreground">:logs</span>
        </span>

        <div className="w-px h-4 bg-accent" />

        {/* Event counter */}
        <span className="font-mono text-[11px] text-muted-foreground tabular-nums">
          {lines.length}
          {lines.length !== rawLines.length && (
            <span className="text-muted-foreground"> / {rawLines.length}</span>
          )}
          {" "}events
        </span>

        {/* Status dot */}
        {error ? (
          <span className="text-[10px] text-red-400">
            ● {notEnabled ? "event log not enabled (pass VALORI_EVENT_LOG_PATH)" : "backend unreachable"}
          </span>
        ) : isLoading ? (
          <span className="text-[10px] text-muted-foreground">loading…</span>
        ) : (
          <span className="text-[10px] text-emerald-500">● connected</span>
        )}

        <div className="flex-1" />

        {/* Controls */}
        <button
          onClick={() => setLiveRefresh((v) => !v)}
          className={`text-[10px] px-2 py-1 rounded border transition-all ${
            liveRefresh
              ? "border-emerald-300 bg-emerald-100 text-emerald-700 dark:border-emerald-800 dark:bg-emerald-950/50 dark:text-emerald-400"
              : "border-input text-muted-foreground hover:text-accent-foreground"
          }`}
        >
          {liveRefresh ? "⏸ live" : "▶ live"}
        </button>

        <button
          onClick={() => {
            setAutoScroll(true);
            bottomRef.current?.scrollIntoView({ behavior: "smooth" });
          }}
          className={`text-[10px] px-2 py-1 rounded border transition-all ${
            autoScroll
              ? "border-sky-300 bg-sky-100 text-sky-700 dark:border-sky-800 dark:bg-sky-950/50 dark:text-sky-400"
              : "border-input text-muted-foreground hover:text-accent-foreground"
          }`}
        >
          ↓ tail
        </button>

        {/* Jump to first error */}
        {rawLines.some(l => /error|ERROR|panic|PANIC|failed|FAILED/i.test(l)) && (
          <button
            onClick={() => {
              setFilter("error");
              setTimeout(() => {
                const el = containerRef.current;
                if (el) el.scrollTop = 0;
              }, 50);
            }}
            className="text-[10px] px-2 py-1 rounded border border-red-500/30 bg-red-500/10 text-red-500 hover:bg-red-500/20 transition-colors"
          >
            ⚠ errors
          </button>
        )}

        <CopyBtn text={allText} label="copy all" />
      </div>

      {/* -- Filter row -- */}
      <div className="flex-shrink-0 flex items-center gap-2 px-4 py-2 border-b border-border/60 bg-background/80 flex-wrap">
        {/* Text filter */}
        <div className="relative flex-shrink-0">
          <span className="absolute left-2.5 top-1/2 -translate-y-1/2 text-muted-foreground font-mono text-[11px]">
            /
          </span>
          <input
            type="text"
            value={filter}
            onChange={(e) => setFilter(e.target.value)}
            placeholder="filter…"
            className="font-mono text-[12px] bg-card border border-input rounded px-3 py-1 pl-6 w-52 text-card-foreground placeholder:text-muted-foreground outline-none focus:border-ring transition-colors"
          />
          {filter && (
            <button
              onClick={() => setFilter("")}
              className="absolute right-2 top-1/2 -translate-y-1/2 text-muted-foreground hover:text-muted-foreground text-xs"
            >
              ×
            </button>
          )}
        </div>

        {/* Kind toggles */}
        <div className="flex items-center gap-1 flex-wrap">
          {allKinds.map((k) => (
            <button
              key={k}
              onClick={() => toggleKind(k)}
              style={kinds.has(k) || kinds.size === 0
                ? { color: lineColor(`: ${k}`) }
                : undefined}
              className={`font-mono text-[10px] px-1.5 py-0.5 rounded border transition-all ${
                kinds.size > 0 && !kinds.has(k)
                  ? "border-border text-muted-foreground opacity-50"
                  : "border-input hover:border-ring"
              }`}
            >
              {k}
            </button>
          ))}
          {kinds.size > 0 && (
            <button
              onClick={() => setKinds(new Set())}
              className="text-[10px] text-muted-foreground hover:text-muted-foreground ml-1"
            >
              clear
            </button>
          )}
        </div>
      </div>

      {/* -- Log body -- */}
      <div
        ref={containerRef}
        onScroll={handleScroll}
        className="flex-1 overflow-y-auto"
      >
        {/* No event log configured, or backend unreachable */}
        {error && (
          <div className="flex flex-col items-center justify-center h-full gap-3 text-center">
            <span className="font-mono text-muted-foreground text-sm">
              {notEnabled ? "event log not enabled" : "backend unreachable — is the node running?"}
            </span>
            {notEnabled && (
              <code className="font-mono text-[11px] text-muted-foreground bg-card border border-border rounded px-3 py-2">
                VALORI_EVENT_LOG_PATH=/tmp/valori-events.log cargo run -p valori-node
              </code>
            )}
          </div>
        )}

        {/* Empty state */}
        {!error && !isLoading && lines.length === 0 && (
          <div className="flex items-center justify-center h-full">
            <span className="font-mono text-muted-foreground text-sm">
              {filter || kinds.size > 0 ? "no matching events" : "no events yet"}
            </span>
          </div>
        )}

        {/* Log lines */}
        {!error && lines.map((line, i) => (
          <LogLine key={i} index={i} line={line} />
        ))}

        {/* Scroll anchor */}
        <div ref={bottomRef} />
      </div>

      {/* -- Status bar -- */}
      <div className="flex-shrink-0 flex items-center gap-4 px-4 py-1.5 border-t border-border bg-background/80 font-mono text-[10px] text-muted-foreground">
        <span>
          {autoScroll ? "↓ tailing" : "scroll ↑ — click tail to resume"}
        </span>
        <span className="flex-1" />
        <span>refreshes every 2 s when live</span>
        <span className="w-px h-3 bg-accent" />
        <span>
          {lines.length} shown / {rawLines.length} total
        </span>
      </div>
    </div>
  );
}
