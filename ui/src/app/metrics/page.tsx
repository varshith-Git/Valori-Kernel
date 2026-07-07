"use client";

import React, { useState, useEffect, useRef, useCallback } from "react";
import useSWR from "swr";

// -- Constants -----------------------------------------------------------------
const HISTORY = 60;       // data points to retain
const FAST_MS = 2_000;    // health + latency poll
const SNAP_MS = 30_000;   // snapshot poll
const FILES_MS = 10_000;  // local files poll

// -- Types ---------------------------------------------------------------------
interface HealthSnapshot {
  dim: number;
  status: string;
  version: string;
  index: string;
  persistence: string;
  records: { live: number; capacity: number; fill_pct: number };
  nodes: { live: number; capacity: number; fill_pct: number };
  edges: { live: number; capacity: number; fill_pct: number };
  event_log_height: number | null;
}

interface PingResult {
  latency_ms: number;
  search_ok: boolean;
  has_records: boolean;
  health: HealthSnapshot;
  error?: string;
}

interface SnapshotEntry { epoch_secs: number; size_bytes: number }
interface LocalFile { kind: "snap" | "log"; size_bytes: number; name: string }

interface Series {
  ts: number;
  value: number;
}

function push(arr: Series[], value: number): Series[] {
  return [...arr, { ts: Date.now(), value }].slice(-HISTORY);
}

// -- Helpers -------------------------------------------------------------------
const fetcher = (url: string) => fetch(url).then((r) => r.json());

function fmtBytes(b: number) {
  if (b < 1024) return `${b} B`;
  if (b < 1_048_576) return `${(b / 1024).toFixed(1)} KB`;
  return `${(b / 1_048_576).toFixed(2)} MB`;
}

function fmtAge(epochSecs: number) {
  const secs = Math.floor(Date.now() / 1000) - epochSecs;
  if (secs < 60) return `${secs}s ago`;
  if (secs < 3600) return `${Math.floor(secs / 60)}m ago`;
  if (secs < 86400) return `${Math.floor(secs / 3600)}h ${Math.floor((secs % 3600) / 60)}m ago`;
  return `${Math.floor(secs / 86400)}d ago`;
}

function fmtRate(r: number) {
  if (r === 0) return "0";
  if (r < 0.1) return r.toFixed(3);
  if (r < 10)  return r.toFixed(2);
  return r.toFixed(1);
}

// -- Sparkline SVG -------------------------------------------------------------
const Sparkline = React.memo(function Sparkline({
  data,
  color,
  fillOpacity = 0.12,
  height = 56,
}: {
  data: Series[];
  color: string;
  fillOpacity?: number;
  height?: number;
}) {
  if (data.length < 2) {
    return (
      <svg width="100%" height={height} viewBox="0 0 280 56" preserveAspectRatio="none">
        <line x1="0" y1={height / 2} x2="280" y2={height / 2} stroke="var(--border)" strokeWidth="1" />
      </svg>
    );
  }

  const W = 280;
  const H = height;
  const pad = 4;
  const vals = data.map((d) => d.value);
  const minV = Math.min(...vals);
  const maxV = Math.max(...vals);
  const range = maxV - minV || 1;

  const px = (i: number) => (i / (data.length - 1)) * W;
  const py = (v: number) =>
    H - pad - ((v - minV) / range) * (H - pad * 2);

  const linePts = vals.map((v, i) => `${px(i)},${py(v)}`).join(" ");
  const areaPts = `0,${H} ${linePts} ${W},${H}`;

  const lx = px(vals.length - 1);
  const ly = py(vals[vals.length - 1]);

  // Use color-mix so we can pass CSS variables as color
  const fill = `color-mix(in srgb, ${color} ${Math.round(fillOpacity * 100)}%, transparent)`;

  return (
    <svg width="100%" height={H} viewBox={`0 0 ${W} ${H}`} preserveAspectRatio="none" className="transition-all duration-300">
      <polygon points={areaPts} fill={fill} className="transition-all duration-300" />
      <polyline
        points={linePts}
        fill="none"
        stroke={color}
        strokeWidth="1.5"
        strokeLinejoin="round"
        strokeLinecap="round"
        className="transition-all duration-300"
      />
      <circle cx={lx} cy={ly} r="3" fill={color} className="transition-all duration-300 drop-shadow-md" style={{ filter: `drop-shadow(0 0 4px ${color})` }} />
    </svg>
  );
});

// -- Metric card ---------------------------------------------------------------
function MetricCard({
  label,
  value,
  unit,
  sub,
  series,
  color,
  trend,
  alert,
}: {
  label: string;
  value: string;
  unit?: string;
  sub?: string;
  series: Series[];
  color: string;
  trend?: "up" | "down" | "flat";
  alert?: boolean;
}) {
  const trendIcon = trend === "up" ? "↑" : trend === "down" ? "↓" : "→";
  const trendColor =
    trend === "up" ? "#4ade80" : trend === "down" ? "#f87171" : "#71717a";

  const vals = series.map((d) => d.value);
  const minV = vals.length ? Math.min(...vals) : 0;
  const maxV = vals.length ? Math.max(...vals) : 0;

  return (
    <div
      className={`rounded-xl border overflow-hidden flex flex-col transition-all duration-300 hover:-translate-y-1 hover:shadow-xl ${
        alert 
          ? "border-destructive/50 bg-destructive/10 shadow-[0_0_15px_rgba(239,68,68,0.2)]" 
          : "border-border/50 bg-card/40 backdrop-blur-md shadow-[0_4px_24px_rgba(0,0,0,0.2)] hover:border-border"
      }`}
    >
      {/* Top row */}
      <div className="flex items-center justify-between px-4 pt-4 pb-1">
        <span className="text-[11px] text-muted-foreground uppercase tracking-widest">{label}</span>
        {trend && (
          <span className="text-[11px] font-mono" style={{ color: trendColor }}>
            {trendIcon}
          </span>
        )}
      </div>

      {/* Value */}
      <div className="flex items-baseline gap-1.5 px-4 pb-3">
        <span
          className={`font-mono text-2xl font-semibold tabular-nums ${alert ? "text-destructive" : ""}`}
          style={!alert ? { color } : undefined}
        >
          {value}
        </span>
        {unit && (
          <span className="text-xs text-muted-foreground font-mono">{unit}</span>
        )}
        {sub && (
          <span className="text-[10px] text-muted-foreground ml-1">{sub}</span>
        )}
      </div>

      {/* Sparkline */}
      <div className="px-0 pb-0 flex-1 flex flex-col justify-end">
        <Sparkline data={series} color={alert ? "var(--color-destructive)" : color} />
      </div>

      {/* Min / max */}
      <div className="flex items-center justify-between px-4 py-1.5 border-t border-border">
        <span className="text-[10px] text-muted-foreground font-mono tabular-nums">
          min {fmtRate(minV)}
        </span>
        <span className="text-[10px] text-muted-foreground font-mono tabular-nums">
          max {fmtRate(maxV)}
        </span>
      </div>
    </div>
  );
}

// -- Info card -----------------------------------------------------------------
function InfoCard({
  label,
  value,
  sub,
  icon,
  accent,
}: {
  label: string;
  value: string;
  sub?: string;
  icon: string;
  accent?: "green" | "amber" | "red" | "blue";
}) {
  const accentColor = {
    green: "var(--color-emerald-500)",
    amber: "var(--color-amber-500)",
    red:   "var(--color-red-500)",
    blue:  "var(--color-sky-500)",
  }[accent ?? "blue"];

  return (
    <div className="rounded-xl border border-border bg-background px-5 py-4 flex items-center gap-4">
      <span
        className="text-xl flex-shrink-0 w-9 h-9 flex items-center justify-center rounded-lg border border-border bg-card"
        style={{ color: accentColor }}
      >
        {icon}
      </span>
      <div className="flex-1 min-w-0">
        <p className="text-[10px] text-muted-foreground uppercase tracking-widest">{label}</p>
        <p className="text-sm font-semibold text-foreground mt-0.5" style={{ color: accentColor }}>
          {value}
        </p>
        {sub && <p className="text-[10px] text-muted-foreground mt-0.5">{sub}</p>}
      </div>
    </div>
  );
}

// -- Fill gauge ----------------------------------------------------------------
function FillBar({ label, pct, color }: { label: string; pct: number; color: string }) {
  return (
    <div className="flex flex-col gap-1">
      <div className="flex items-center justify-between">
        <span className="text-[11px] text-muted-foreground">{label}</span>
        <span className="text-[11px] font-mono text-muted-foreground">{pct.toFixed(1)}%</span>
      </div>
      <div className="h-1 rounded-full bg-accent overflow-hidden">
        <div
          className="h-full rounded-full transition-all duration-500"
          style={{
            width: `${Math.min(pct, 100)}%`,
            background:
              pct >= 90 ? "#f87171" : pct >= 70 ? "#fbbf24" : color,
          }}
        />
      </div>
    </div>
  );
}

// -- Main page -----------------------------------------------------------------
export default function MetricsPage() {
  // -- Time series state ------------------------------------------------------
  const [latSeries,    setLatSeries]    = useState<Series[]>([]);
  const [recSeries,    setRecSeries]    = useState<Series[]>([]);
  const [insertSeries, setInsertSeries] = useState<Series[]>([]);
  const [evtSeries,    setEvtSeries]    = useState<Series[]>([]);

  // For rate derivation
  const prevRecs   = useRef<number | null>(null);
  const prevEvts   = useRef<number | null>(null);
  const prevTs     = useRef<number | null>(null);

  // Latest health snapshot
  const [health, setHealth] = useState<HealthSnapshot | null>(null);
  const [lastPing, setLastPing] = useState<number | null>(null);
  const [connected, setConnected] = useState(true);

  // -- Fast poll: ping (latency + health) ------------------------------------
  const poll = useCallback(async () => {
    try {
      const res = await fetch("/api/metrics/ping", { cache: "no-store" });
      if (!res.ok) { setConnected(false); return; }
      const d = await res.json() as PingResult;
      if (d.error) { setConnected(false); return; }

      setConnected(true);
      setLastPing(Date.now());
      setHealth(d.health);

      const now = Date.now();
      const elapsed = prevTs.current ? (now - prevTs.current) / 1000 : null;

      // Insert rate = delta records / elapsed
      const recs = d.health.records.live;
      const evts = d.health.event_log_height ?? 0;

      let insertRate = 0;
      let evtRate = 0;
      if (elapsed && elapsed > 0) {
        if (prevRecs.current !== null) {
          insertRate = Math.max(0, (recs - prevRecs.current) / elapsed);
        }
        if (prevEvts.current !== null) {
          evtRate = Math.max(0, (evts - prevEvts.current) / elapsed);
        }
      }

      prevRecs.current = recs;
      prevEvts.current = evts;
      prevTs.current = now;

      setLatSeries((s) => push(s, d.latency_ms));
      setRecSeries((s) => push(s, recs));
      setInsertSeries((s) => push(s, insertRate));
      setEvtSeries((s) => push(s, evtRate));
    } catch {
      setConnected(false);
    }
  }, []);

  useEffect(() => {
    poll();
    const id = setInterval(poll, FAST_MS);
    return () => clearInterval(id);
  }, [poll]);

  // -- Snapshot poll ----------------------------------------------------------
  const { data: snapsData } = useSWR<{ snapshots: SnapshotEntry[]; disabled?: boolean }>(
    "/api/storage/snapshots",
    fetcher,
    { refreshInterval: SNAP_MS }
  );

  // -- Local files poll -------------------------------------------------------
  const { data: filesData } = useSWR<{ files: LocalFile[] }>(
    "/api/local-files",
    fetcher,
    { refreshInterval: FILES_MS }
  );

  // -- Derived values ---------------------------------------------------------
  const latCurrent   = latSeries.at(-1)?.value ?? 0;
  const recCurrent   = recSeries.at(-1)?.value ?? 0;
  const insCurrent   = insertSeries.at(-1)?.value ?? 0;
  const evtCurrent   = evtSeries.at(-1)?.value ?? 0;

  const latAlert = latCurrent > 500;
  const latAmber = latCurrent > 100;

  const latColor = latAlert ? "#f87171" : latAmber ? "#fbbf24" : "#38bdf8";

  const latTrend = latSeries.length >= 3
    ? latSeries.at(-1)!.value > latSeries.at(-3)!.value ? "up"
    : latSeries.at(-1)!.value < latSeries.at(-3)!.value ? "down" : "flat"
    : undefined;

  const snapshots = snapsData?.snapshots ?? [];
  const newestSnap = snapshots.sort((a, b) => b.epoch_secs - a.epoch_secs)[0];

  const walFiles = (filesData?.files ?? []).filter((f) => f.kind === "log");
  const walBytes = walFiles.reduce((s, f) => s + f.size_bytes, 0);

  const snapFiles = (filesData?.files ?? []).filter((f) => f.kind === "snap");
  const snapBytes = snapFiles.reduce((s, f) => s + f.size_bytes, 0);

  const statusColor =
    health?.status === "ok" ? "var(--color-emerald-500)"
    : health?.status === "degraded" ? "var(--color-amber-500)"
    : "var(--color-red-500)";

  return (
    <div className="flex flex-col gap-6 w-full max-w-[1600px]">

      {/* -- Header -- */}
      <div className="flex items-center gap-3 flex-wrap">
        <div>
          <h1 className="text-lg font-semibold text-foreground">Metrics</h1>
          <p className="text-xs text-muted-foreground mt-0.5">
            Live — refreshes every {FAST_MS / 1000}s · last {HISTORY} data points shown
          </p>
        </div>
        <div className="ml-auto flex items-center gap-3">
          <span
            className="flex items-center gap-1.5 text-[11px] font-mono"
            style={{ color: connected ? "#4ade80" : "#f87171" }}
          >
            <span
              className="w-1.5 h-1.5 rounded-full"
              style={{
                background: connected ? "#4ade80" : "#f87171",
                animation: connected ? "pulse 2s infinite" : "none",
              }}
            />
            {connected ? "connected" : "disconnected"}
          </span>
          {lastPing && (
            <span className="text-[10px] text-muted-foreground font-mono tabular-nums">
              {new Date(lastPing).toLocaleTimeString(undefined, { hour: "2-digit", minute: "2-digit", second: "2-digit" })}
            </span>
          )}
        </div>
      </div>

      {/* -- Sparkline grid -- */}
      <div className="grid grid-cols-2 lg:grid-cols-4 gap-3">
        <MetricCard
          label="Search latency"
          value={latCurrent > 0 ? String(latCurrent) : "—"}
          unit="ms"
          sub={latAlert ? "critical" : latAmber ? "elevated" : undefined}
          series={latSeries}
          color={latColor}
          trend={latTrend as "up" | "down" | "flat" | undefined}
          alert={latAlert}
        />
        <MetricCard
          label="Inserts / sec"
          value={fmtRate(insCurrent)}
          unit="/s"
          series={insertSeries}
          color="var(--color-emerald-500)"
          trend={
            insertSeries.length >= 3
              ? insertSeries.at(-1)!.value > insertSeries.at(-3)!.value ? "up"
              : insertSeries.at(-1)!.value < insertSeries.at(-3)!.value ? "down" : "flat"
              : undefined
          }
        />
        <MetricCard
          label="Total records"
          value={recCurrent.toLocaleString()}
          series={recSeries}
          color="var(--color-violet-500)"
        />
        <MetricCard
          label="Event rate"
          value={fmtRate(evtCurrent)}
          unit="ev/s"
          sub={health?.event_log_height !== null && health?.event_log_height !== undefined
            ? `${health.event_log_height.toLocaleString()} total`
            : "log off"}
          series={evtSeries}
          color="var(--color-amber-500)"
        />
      </div>

      {/* -- Status row -- */}
      <div className="grid grid-cols-2 lg:grid-cols-3 gap-3">
        <InfoCard
          icon="◉"
          label="Latest snapshot"
          value={newestSnap ? fmtAge(newestSnap.epoch_secs) : snapsData?.disabled ? "object store off" : "none yet"}
          sub={newestSnap ? fmtBytes(newestSnap.size_bytes) : undefined}
          accent={!newestSnap ? "amber" : "green"}
        />
        <InfoCard
          icon="▤"
          label="WAL size (local)"
          value={walBytes > 0 ? fmtBytes(walBytes) : "—"}
          sub={walFiles.length > 0 ? `${walFiles.length} log file${walFiles.length !== 1 ? "s" : ""}` : "no .log files found"}
          accent="blue"
        />
        <InfoCard
          icon="▲"
          label="Snap files (local)"
          value={snapBytes > 0 ? fmtBytes(snapBytes) : "—"}
          sub={snapFiles.length > 0 ? `${snapFiles.length} .snap file${snapFiles.length !== 1 ? "s" : ""}` : "no .snap files"}
          accent="blue"
        />
      </div>

      {/* -- Slab fill gauges + server info -- */}
      <div className="grid grid-cols-1 lg:grid-cols-2 gap-4">

        {/* Slab utilisation */}
        <div className="rounded-xl border border-border bg-background px-5 py-4 flex flex-col gap-4">
          <p className="text-[10px] text-muted-foreground uppercase tracking-widest">Slab utilisation</p>
          {health ? (
            health.records ? (
              <>
                <FillBar label={`Records — ${health.records.live.toLocaleString()} / ${health.records.capacity.toLocaleString()}`} pct={health.records.fill_pct} color="var(--color-violet-500)" />
                <FillBar label={`Nodes — ${health.nodes.live.toLocaleString()} / ${health.nodes.capacity.toLocaleString()}`} pct={health.nodes.fill_pct} color="var(--color-sky-500)" />
                <FillBar label={`Edges — ${health.edges.live.toLocaleString()} / ${health.edges.capacity.toLocaleString()}`} pct={health.edges.fill_pct} color="var(--color-emerald-500)" />
              </>
            ) : (
              <p className="text-xs text-muted-foreground">Slab metrics unavailable in cluster mode.</p>
            )
          ) : (
            <p className="text-xs text-muted-foreground">Waiting for data…</p>
          )}
        </div>

        {/* Server info */}
        <div className="rounded-xl border border-border bg-background px-5 py-4 flex flex-col gap-3">
          <p className="text-[10px] text-muted-foreground uppercase tracking-widest">Server info</p>
          {health ? (
            <div className="grid grid-cols-2 gap-x-6 gap-y-3">
              {[
                { k: "Status", v: health.status, color: statusColor },
                { k: "Version", v: health.version, color: "#71717a" },
                { k: "Dimension", v: String(health.dim), color: "#71717a" },
                { k: "Index", v: health.index, color: "#71717a" },
                { k: "Persistence", v: health.persistence, color: "#71717a" },
                { k: "Nodes", v: health.nodes?.live?.toLocaleString() ?? "N/A", color: "#38bdf8" },
                { k: "Edges", v: health.edges?.live?.toLocaleString() ?? "N/A", color: "#4ade80" },
                { k: "Events", v: health.event_log_height !== undefined && health.event_log_height !== null ? health.event_log_height!.toLocaleString() : "—", color: "#fbbf24" },
              ].map(({ k, v, color }) => (
                <div key={k} className="flex flex-col gap-0.5">
                  <span className="text-[10px] text-muted-foreground">{k}</span>
                  <span className="text-xs font-mono font-medium" style={{ color }}>{v}</span>
                </div>
              ))}
            </div>
          ) : (
            <p className="text-xs text-muted-foreground">Connecting…</p>
          )}
        </div>
      </div>

      <style>{`
        @keyframes pulse {
          0%, 100% { opacity: 1; }
          50% { opacity: 0.4; }
        }
      `}</style>
    </div>
  );
}
