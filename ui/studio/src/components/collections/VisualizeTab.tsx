"use client";

import { useState, useRef, useEffect, useCallback } from "react";
import { Button } from "@/components/ui/button";
import { RefreshCw, Maximize2, MoreHorizontal, HelpCircle, Plus, Minus, Crosshair, Box } from "lucide-react";
import { useTransport } from "@/runtime/context";
import type { Transport } from "@/runtime/transport";

// ─── Math helpers ─────────────────────────────────────────────────────────────

function pca2dWithVariance(vecs: number[][]): {
  proj1: number[]; proj2: number[]; pct1: number; pct2: number;
} {
  const n = vecs.length;
  const d = vecs[0]?.length ?? 0;
  if (n < 2 || d === 0) return { proj1: [], proj2: [], pct1: 0, pct2: 0 };

  const mean = Array(d).fill(0) as number[];
  for (const v of vecs) for (let j = 0; j < d; j++) mean[j] += v[j] / n;
  const centered = vecs.map((v) => v.map((x, j) => x - mean[j]));

  const totalVar =
    centered.reduce((s, v) => s + v.reduce((ss, x) => ss + x * x, 0), 0) || 1;

  function powerIter(data: number[][], exclude?: number[]): number[] {
    let u: number[] = Array(d).fill(0).map((_, i) => (i === 0 ? 1 : 0));
    for (let iter = 0; iter < 30; iter++) {
      const Xu = data.map((row) => row.reduce((s, x, j) => s + x * u[j], 0));
      let next = Array(d).fill(0) as number[];
      for (let i = 0; i < data.length; i++)
        for (let j = 0; j < d; j++) next[j] += Xu[i] * data[i][j];
      if (exclude) {
        const dot = next.reduce((s, x, j) => s + x * exclude[j], 0);
        next = next.map((x, j) => x - dot * exclude[j]);
      }
      const norm = Math.sqrt(next.reduce((s, x) => s + x * x, 0)) || 1;
      u = next.map((x) => x / norm);
    }
    return u;
  }

  const pc1 = powerIter(centered);
  const pc2 = powerIter(centered, pc1);
  const proj1 = centered.map((v) => v.reduce((s, x, j) => s + x * pc1[j], 0));
  const proj2 = centered.map((v) => v.reduce((s, x, j) => s + x * pc2[j], 0));

  const var1 = proj1.reduce((s, x) => s + x * x, 0);
  const var2 = proj2.reduce((s, x) => s + x * x, 0);
  return { proj1, proj2, pct1: (var1 / totalVar) * 100, pct2: (var2 / totalVar) * 100 };
}

function dbscan(xs: number[], ys: number[], eps: number, minPts: number): number[] {
  const n = xs.length;
  const labels = new Array<number>(n).fill(-2);
  let clusterId = -1;

  function neighbors(i: number): number[] {
    const r: number[] = [];
    for (let j = 0; j < n; j++)
      if (Math.hypot(xs[i] - xs[j], ys[i] - ys[j]) <= eps) r.push(j);
    return r;
  }

  for (let i = 0; i < n; i++) {
    if (labels[i] !== -2) continue;
    const nb = neighbors(i);
    if (nb.length < minPts) { labels[i] = -1; continue; }
    clusterId++;
    labels[i] = clusterId;
    const queue = nb.filter((j) => j !== i);
    while (queue.length > 0) {
      const j = queue.shift()!;
      if (labels[j] === -1) labels[j] = clusterId;
      if (labels[j] !== -2) continue;
      labels[j] = clusterId;
      const nb2 = neighbors(j);
      if (nb2.length >= minPts) queue.push(...nb2.filter((k) => labels[k] === -2));
    }
  }
  return labels;
}

function niceTicks(min: number, max: number, count = 5): number[] {
  const range = max - min || 1;
  const rawStep = range / count;
  const mag = Math.pow(10, Math.floor(Math.log10(rawStep)));
  const step = Math.ceil(rawStep / mag) * mag;
  const start = Math.ceil(min / step) * step;
  const ticks: number[] = [];
  for (let t = start; t <= max + step * 0.01; t += step)
    ticks.push(parseFloat(t.toFixed(10)));
  return ticks;
}

// ─── Types ────────────────────────────────────────────────────────────────────

interface Point { id: number; x: number; y: number; score: number; cluster: number; }
interface Props  { projectId: string; namespace: string; dim: number | null; }

// ─── Constants ────────────────────────────────────────────────────────────────

const CLUSTER_COLORS = [
  "#818cf8", "#34d399", "#fb923c", "#f472b6",
  "#38bdf8", "#a78bfa", "#fbbf24",
];
const BATCH_SIZE  = 20;
const MAX_POINTS  = 200;
const PAD_LEFT    = 58;
const PAD_BOTTOM  = 44;
const PAD_TOP     = 16;
const PAD_RIGHT   = 16;

// ─── Batch fetch ──────────────────────────────────────────────────────────────

async function fetchVectorsBatched(
  transport: Transport,
  ids: number[], qs: string,
  onProgress: (done: number, total: number) => void,
  projectId: string,
): Promise<({ id: number; vector: number[] } | null)[]> {
  const results: ({ id: number; vector: number[] } | null)[] = [];
  for (let i = 0; i < ids.length; i += BATCH_SIZE) {
    const batch = ids.slice(i, i + BATCH_SIZE);
    const batchResults = await Promise.all(
      batch.map((id) =>
        fetch(transport.path(projectId, `/records/${id}${qs}`))
          .then((r) => r.json() as Promise<{ id: number; vector: number[] }>)
          .catch(() => null),
      ),
    );
    results.push(...batchResults);
    onProgress(Math.min(i + BATCH_SIZE, ids.length), ids.length);
  }
  return results;
}

// ─── Viewport helpers ─────────────────────────────────────────────────────────

function makeViewport(points: Point[], zoom: number) {
  const xs = points.map((p) => p.x), ys = points.map((p) => p.y);
  const rawMinX = Math.min(...xs), rawMaxX = Math.max(...xs);
  const rawMinY = Math.min(...ys), rawMaxY = Math.max(...ys);
  const cx = (rawMinX + rawMaxX) / 2, cy = (rawMinY + rawMaxY) / 2;
  const halfX = ((rawMaxX - rawMinX) * 0.56 + 0.5) / zoom;
  const halfY = ((rawMaxY - rawMinY) * 0.56 + 0.5) / zoom;
  const dataMinX = cx - halfX, dataMaxX = cx + halfX;
  const dataMinY = cy - halfY, dataMaxY = cy + halfY;
  const plotW = 820 - PAD_LEFT - PAD_RIGHT;
  const plotH = 480 - PAD_TOP - PAD_BOTTOM;
  const toX = (x: number) => PAD_LEFT + ((x - dataMinX) / (dataMaxX - dataMinX)) * plotW;
  const toY = (y: number) => PAD_TOP + plotH - ((y - dataMinY) / (dataMaxY - dataMinY)) * plotH;
  return { dataMinX, dataMaxX, dataMinY, dataMaxY, toX, toY };
}

// ─── Sub-components ───────────────────────────────────────────────────────────

function Toggle({ checked, onChange }: { checked: boolean; onChange: (v: boolean) => void }) {
  return (
    <button
      role="switch"
      aria-checked={checked}
      onClick={() => onChange(!checked)}
      style={{
        width: 36, height: 20, borderRadius: 10,
        background: checked ? "var(--v-accent)" : "var(--border)",
        border: "none", cursor: "pointer", position: "relative",
        transition: "background 0.15s", flexShrink: 0,
      }}
    >
      <span style={{
        position: "absolute", top: 2, left: checked ? 18 : 2,
        width: 16, height: 16, borderRadius: "50%", background: "white",
        transition: "left 0.15s", display: "block",
      }} />
    </button>
  );
}

function ControlField({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 5 }}>
      <span style={{ fontSize: 11, color: "var(--muted-foreground)", fontWeight: 500 }}>{label}</span>
      {children}
    </div>
  );
}

// ─── Main component ───────────────────────────────────────────────────────────

export function VisualizeTab({ projectId, namespace, dim }: Props) {
  const transport = useTransport();
  const [points,       setPoints]       = useState<Point[]>([]);
  const [totalRecords, setTotalRecords] = useState<number | null>(null);
  const [pct1,         setPct1]         = useState(0);
  const [pct2,         setPct2]         = useState(0);
  const [clusterCount, setClusterCount] = useState(0);
  const [avgDist,      setAvgDist]      = useState(0);
  const [loading,      setLoading]      = useState(false);
  const [progress,     setProgress]     = useState<{ done: number; total: number } | null>(null);
  const [error,        setError]        = useState<string | null>(null);
  const [hovered,      setHovered]      = useState<Point | null>(null);
  const [hoveredPos,   setHoveredPos]   = useState<{ x: number; y: number } | null>(null);
  const [activeTab,    setActiveTab]    = useState<"controls" | "legend">("controls");
  const [pointSize,    setPointSize]    = useState(6);
  const [opacity,      setOpacity]      = useState(80);
  const [showGrid,     setShowGrid]     = useState(true);
  const [showLabels,   setShowLabels]   = useState(false);
  const [showCluster,  setShowCluster]  = useState(true);
  const [zoom,         setZoom]         = useState(1);
  const [menuOpen,     setMenuOpen]     = useState(false);

  const canvasRef = useRef<HTMLCanvasElement>(null);

  // Scrollpad/wheel zoom. React attaches its synthetic wheel listener as
  // passive by default, so e.preventDefault() in an onWheel prop wouldn't
  // actually stop the page from scrolling — a native listener with
  // { passive: false } is required to trade the pad gesture for zoom.
  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;
    const onWheel = (e: WheelEvent) => {
      e.preventDefault();
      const factor = Math.exp(-e.deltaY * 0.001);
      setZoom((z) => Math.min(Math.max(z * factor, 0.2), 5));
    };
    canvas.addEventListener("wheel", onWheel, { passive: false });
    return () => canvas.removeEventListener("wheel", onWheel);
  }, []);

  // ── Load ──────────────────────────────────────────────────────────────────

  const load = useCallback(async () => {
    if (dim == null) { setError("Vector dimension not known yet"); return; }
    setLoading(true); setProgress(null); setError(null);
    try {
      const nsRes = await fetch(transport.path(projectId, `/namespaces`)).catch(() => null);
      if (nsRes?.ok) {
        const nsData = await nsRes.json() as { namespaces?: { name: string; record_count?: number }[] };
        const ns = nsData.namespaces?.find((n) => n.name === namespace);
        setTotalRecords(ns?.record_count ?? null);
      }

      const res = await fetch(transport.path(projectId, `/search`), {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ query: Array(dim).fill(0), k: MAX_POINTS, collection: namespace }),
      });
      if (!res.ok) throw new Error(`Search failed (${res.status})`);
      const data = await res.json() as { results: { id: number; score: number }[] };
      const results = data.results ?? [];
      const qs = namespace ? `?collection=${encodeURIComponent(namespace)}` : "";

      const fetched = await fetchVectorsBatched(
        transport,
        results.map((r) => r.id), qs,
        (done, total) => setProgress({ done, total }),
        projectId,
      );
      setProgress(null);
      const vecs = fetched.filter(Boolean) as { id: number; vector: number[] }[];
      if (vecs.length < 2) { setError("Need at least 2 records to visualize"); return; }

      const { proj1, proj2, pct1: p1, pct2: p2 } = pca2dWithVariance(vecs.map((v) => v.vector));
      setPct1(p1); setPct2(p2);

      // Auto-epsilon DBSCAN
      const sample = Math.min(vecs.length, 50);
      let nnSum = 0;
      for (let i = 0; i < sample; i++) {
        let minD = Infinity;
        for (let j = 0; j < proj1.length; j++) {
          if (i === j) continue;
          const d = Math.hypot(proj1[i] - proj1[j], proj2[i] - proj2[j]);
          if (d < minD) minD = d;
        }
        nnSum += minD;
      }
      const eps = (nnSum / sample) * 3;
      const labels = dbscan(proj1, proj2, eps, 3);
      setClusterCount(Math.max(0, ...labels) + 1);

      // Avg distance (sample)
      let distSum = 0, distCount = 0;
      for (let i = 0; i < Math.min(proj1.length, 30); i++) {
        for (let j = i + 1; j < Math.min(proj1.length, 30); j++) {
          distSum += Math.hypot(proj1[i] - proj1[j], proj2[i] - proj2[j]);
          distCount++;
        }
      }
      setAvgDist(distCount > 0 ? distSum / distCount : 0);

      setPoints(vecs.map((v, i) => ({
        id: v.id, x: proj1[i], y: proj2[i],
        score: results.find((r) => r.id === v.id)?.score ?? 0,
        cluster: labels[i],
      })));
      setZoom(1);
    } catch (e) {
      setProgress(null);
      setError(e instanceof Error ? e.message : "Failed to load");
    } finally {
      setLoading(false);
    }
  }, [dim, namespace]);

  // ── Draw ──────────────────────────────────────────────────────────────────

  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas || points.length === 0) return;
    const ctx = canvas.getContext("2d");
    if (!ctx) return;

    const W = canvas.width, H = canvas.height;
    const plotW = W - PAD_LEFT - PAD_RIGHT, plotH = H - PAD_TOP - PAD_BOTTOM;
    const { dataMinX, dataMaxX, dataMinY, dataMaxY, toX, toY } = makeViewport(points, zoom);

    const isDark = document.documentElement.classList.contains("dark");
    const bgColor    = isDark ? "#09090b" : "#fafafa";
    const gridColor  = isDark ? "rgba(255,255,255,0.055)" : "rgba(0,0,0,0.055)";
    const crossColor = isDark ? "rgba(255,255,255,0.13)"  : "rgba(0,0,0,0.13)";
    const axisColor  = isDark ? "rgba(255,255,255,0.12)"  : "rgba(0,0,0,0.12)";
    const labelColor = isDark ? "rgba(255,255,255,0.40)"  : "rgba(0,0,0,0.40)";
    const tickColor  = isDark ? "rgba(255,255,255,0.22)"  : "rgba(0,0,0,0.22)";

    ctx.fillStyle = bgColor;
    ctx.fillRect(0, 0, W, H);

    const xTicks = niceTicks(dataMinX, dataMaxX, 5);
    const yTicks = niceTicks(dataMinY, dataMaxY, 5);

    // Grid
    if (showGrid) {
      ctx.strokeStyle = gridColor; ctx.lineWidth = 1;
      for (const t of xTicks) {
        const gx = toX(t);
        ctx.beginPath(); ctx.moveTo(gx, PAD_TOP); ctx.lineTo(gx, H - PAD_BOTTOM); ctx.stroke();
      }
      for (const t of yTicks) {
        const gy = toY(t);
        ctx.beginPath(); ctx.moveTo(PAD_LEFT, gy); ctx.lineTo(W - PAD_RIGHT, gy); ctx.stroke();
      }
    }

    // Center crosshair dashed
    ctx.strokeStyle = crossColor; ctx.lineWidth = 1;
    ctx.setLineDash([4, 5]);
    const cx0 = toX(0), cy0 = toY(0);
    if (cx0 >= PAD_LEFT && cx0 <= W - PAD_RIGHT) {
      ctx.beginPath(); ctx.moveTo(cx0, PAD_TOP); ctx.lineTo(cx0, H - PAD_BOTTOM); ctx.stroke();
    }
    if (cy0 >= PAD_TOP && cy0 <= H - PAD_BOTTOM) {
      ctx.beginPath(); ctx.moveTo(PAD_LEFT, cy0); ctx.lineTo(W - PAD_RIGHT, cy0); ctx.stroke();
    }
    ctx.setLineDash([]);

    // Plot border
    ctx.strokeStyle = axisColor; ctx.lineWidth = 1;
    ctx.strokeRect(PAD_LEFT, PAD_TOP, plotW, plotH);

    // Tick marks + labels
    ctx.font = "11px ui-monospace, monospace";
    ctx.textAlign = "center";
    for (const t of xTicks) {
      const gx = toX(t);
      if (gx < PAD_LEFT || gx > W - PAD_RIGHT) continue;
      ctx.fillStyle = tickColor; ctx.fillRect(gx - 0.5, H - PAD_BOTTOM, 1, 5);
      ctx.fillStyle = labelColor; ctx.fillText(t.toFixed(1), gx, H - PAD_BOTTOM + 15);
    }
    ctx.textAlign = "right";
    for (const t of yTicks) {
      const gy = toY(t);
      if (gy < PAD_TOP || gy > H - PAD_BOTTOM) continue;
      ctx.fillStyle = tickColor; ctx.fillRect(PAD_LEFT - 5, gy - 0.5, 5, 1);
      ctx.fillStyle = labelColor; ctx.fillText(t.toFixed(1), PAD_LEFT - 9, gy + 4);
    }

    // Axis labels
    ctx.fillStyle = isDark ? "rgba(255,255,255,0.32)" : "rgba(0,0,0,0.32)";
    ctx.font = "12px ui-sans-serif, sans-serif";
    ctx.textAlign = "center";
    ctx.fillText(`PC1 (${pct1.toFixed(1)}%)`, PAD_LEFT + plotW / 2, H - 5);
    ctx.save();
    ctx.translate(13, PAD_TOP + plotH / 2);
    ctx.rotate(-Math.PI / 2);
    ctx.fillText(`PC2 (${pct2.toFixed(1)}%)`, 0, 0);
    ctx.restore();

    // Points
    const opA = Math.round((opacity / 100) * 255).toString(16).padStart(2, "0");
    for (const p of points) {
      const px = toX(p.x), py = toY(p.y);
      if (px < PAD_LEFT - pointSize * 2 || px > W - PAD_RIGHT + pointSize * 2) continue;
      if (py < PAD_TOP  - pointSize * 2 || py > H - PAD_BOTTOM + pointSize * 2) continue;
      const isHov = hovered?.id === p.id;
      const r = pointSize / 2 + 2;

      let color = "#818cf8";
      if (showCluster && p.cluster >= 0)
        color = CLUSTER_COLORS[p.cluster % CLUSTER_COLORS.length];

      ctx.beginPath();
      ctx.arc(px, py, isHov ? r + 3 : r, 0, Math.PI * 2);
      ctx.fillStyle = isHov ? "#f59e0b" : color + opA;
      ctx.fill();

      if (isHov) {
        ctx.strokeStyle = "rgba(245,158,11,0.35)"; ctx.lineWidth = 2;
        ctx.beginPath(); ctx.arc(px, py, r + 7, 0, Math.PI * 2); ctx.stroke();
      }
    }

    // Labels
    if (showLabels) {
      ctx.font = "9px ui-monospace, monospace"; ctx.textAlign = "center";
      ctx.fillStyle = labelColor;
      for (const p of points) {
        ctx.fillText(`#${p.id}`, toX(p.x), toY(p.y) - pointSize - 3);
      }
    }
  }, [points, hovered, pointSize, opacity, showGrid, showLabels, showCluster, pct1, pct2, zoom]);

  // ── Mouse ──────────────────────────────────────────────────────────────────

  const handleMouseMove = useCallback((e: React.MouseEvent<HTMLCanvasElement>) => {
    const canvas = canvasRef.current;
    if (!canvas || points.length === 0) return;
    const rect = canvas.getBoundingClientRect();
    const mx = (e.clientX - rect.left) * (canvas.width  / rect.width);
    const my = (e.clientY - rect.top)  * (canvas.height / rect.height);
    const { toX, toY } = makeViewport(points, zoom);

    let closest: Point | null = null;
    let minDist = 14;
    for (const p of points) {
      const d = Math.hypot(toX(p.x) - mx, toY(p.y) - my);
      if (d < minDist) { minDist = d; closest = p; }
    }
    setHovered(closest);
    if (closest) {
      const sx = rect.width  / canvas.width;
      const sy = rect.height / canvas.height;
      setHoveredPos({ x: toX(closest.x) * sx, y: toY(closest.y) * sy });
    } else {
      setHoveredPos(null);
    }
  }, [points, zoom]);

  // ─────────────────────────────────────────────────────────────────────────

  const hasPoints = points.length > 0;

  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 0 }}>
      {/* Header */}
      <div style={{ display: "flex", alignItems: "center", justifyContent: "space-between", marginBottom: 6 }}>
        <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
          <span style={{ fontSize: 15, fontWeight: 600, color: "var(--foreground)" }}>
            2D PCA Projection
          </span>
          <HelpCircle size={14} style={{ color: "var(--muted-foreground)", opacity: 0.55 }} />
        </div>
        <div style={{ display: "flex", alignItems: "center", gap: 6 }}>
          <Button
            size="sm" variant="outline"
            onClick={load}
            disabled={loading || dim == null}
            style={{ gap: 6, fontSize: 13 }}
          >
            <RefreshCw
              size={13}
              style={{ animation: loading ? "spin 1s linear infinite" : "none" }}
            />
            {loading
              ? progress ? `${progress.done}/${progress.total}…` : "Loading…"
              : hasPoints ? "Reload" : "Load"}
          </Button>
          <button style={iconBtnStyle} onClick={() => setZoom(1)} title="Reset zoom"><Maximize2 size={14} /></button>
          <div style={{ position: "relative" }}>
            <button style={iconBtnStyle} onClick={() => setMenuOpen((v) => !v)} title="More options">
              <MoreHorizontal size={14} />
            </button>
            {menuOpen && (
              <>
                <div
                  style={{ position: "fixed", inset: 0, zIndex: 9 }}
                  onClick={() => setMenuOpen(false)}
                />
                <div style={{
                  position: "absolute", top: "calc(100% + 4px)", right: 0, zIndex: 10,
                  background: "var(--background)", border: "1px solid var(--border)",
                  borderRadius: 8, minWidth: 160, padding: 4,
                  boxShadow: "0 4px 16px rgba(0,0,0,0.12)",
                }}>
                  <button
                    style={menuItemStyle}
                    onClick={() => { setZoom(1); setMenuOpen(false); }}
                  >
                    Reset view
                  </button>
                  <button
                    style={menuItemStyle}
                    onClick={() => {
                      const canvas = canvasRef.current;
                      if (canvas) {
                        const a = document.createElement("a");
                        a.download = `${namespace || "visualize"}.png`;
                        a.href = canvas.toDataURL("image/png");
                        a.click();
                      }
                      setMenuOpen(false);
                    }}
                  >
                    Download as PNG
                  </button>
                </div>
              </>
            )}
          </div>
        </div>
      </div>

      <p style={{ fontSize: 12, color: "var(--muted-foreground)", marginBottom: 14, lineHeight: 1.5 }}>
        Projecting up to {MAX_POINTS} vectors into {dim ?? "?"} dimensions. Hover a point to see the record ID.
      </p>

      {error && (
        <div style={{
          borderRadius: 8, border: "1px solid rgba(239,68,68,0.3)",
          background: "rgba(239,68,68,0.08)", padding: "10px 14px",
          fontSize: 13, color: "#f87171", marginBottom: 14,
        }}>
          {error}
        </div>
      )}

      {/* Two-column layout */}
      <div style={{ display: "flex", gap: 12, alignItems: "flex-start" }}>

        {/* Canvas + stats */}
        <div style={{ flex: 1, minWidth: 0 }}>
          {!hasPoints && !loading ? (
            <div style={{
              borderRadius: 12, border: "1px dashed var(--border)",
              padding: "72px 0", textAlign: "center",
            }}>
              <p style={{ fontSize: 13, color: "var(--muted-foreground)" }}>
                Click Load to fetch vectors and render the scatter plot.
              </p>
            </div>
          ) : (
            <div style={{
              position: "relative", borderRadius: 12,
              border: "1px solid var(--border)", overflow: "hidden",
            }}>
              <canvas
                ref={canvasRef}
                width={820}
                height={480}
                style={{ width: "100%", display: "block", cursor: hovered ? "crosshair" : "default" }}
                onMouseMove={handleMouseMove}
                onMouseLeave={() => { setHovered(null); setHoveredPos(null); }}
              />

              {/* Top-right legend */}
              <div style={{
                position: "absolute", top: 12, right: 12,
                fontSize: 11, fontFamily: "ui-monospace, monospace",
                color: "rgba(255,255,255,0.5)",
                background: "rgba(0,0,0,0.38)",
                borderRadius: 6, padding: "3px 9px",
                backdropFilter: "blur(6px)",
                pointerEvents: "none",
              }}>
                {points.length} points · PC1 × PC2
              </div>

              {/* Hover tooltip */}
              {hovered && hoveredPos && (
                <div style={{
                  position: "absolute",
                  left: Math.min(hoveredPos.x + 12, (canvasRef.current?.getBoundingClientRect().width ?? 800) - 160),
                  top: Math.max(hoveredPos.y - 52, 8),
                  background: "var(--card)",
                  border: "1px solid var(--border)",
                  borderRadius: 8,
                  padding: "8px 12px",
                  boxShadow: "0 4px 20px rgba(0,0,0,0.18)",
                  pointerEvents: "none",
                  minWidth: 140,
                  zIndex: 10,
                }}>
                  <div style={{
                    fontSize: 13, fontWeight: 600,
                    color: "var(--foreground)",
                    fontFamily: "ui-monospace, monospace",
                    marginBottom: 5,
                  }}>
                    #{hovered.id}
                  </div>
                  <div style={{
                    fontSize: 12, color: "var(--muted-foreground)",
                    display: "flex", alignItems: "center", gap: 6,
                  }}>
                    <span style={{
                      width: 8, height: 8, borderRadius: "50%",
                      background: "#f59e0b", display: "inline-block", flexShrink: 0,
                    }} />
                    score {hovered.score.toFixed(4)}
                  </div>
                </div>
              )}

              {/* Zoom controls */}
              <div style={{
                position: "absolute", bottom: 12, right: 12,
                display: "flex", flexDirection: "column", gap: 4,
              }}>
                {([
                  { icon: <Plus      size={13} />, action: () => setZoom((z) => Math.min(z * 1.3, 5)) },
                  { icon: <Minus     size={13} />, action: () => setZoom((z) => Math.max(z / 1.3, 0.2)) },
                  { icon: <Maximize2 size={12} />, action: () => setZoom(1) },
                  { icon: <Crosshair size={12} />, action: () => setZoom(1) },
                ] as const).map((btn, i) => (
                  <button key={i} onClick={btn.action} style={zoomBtnStyle}>
                    {btn.icon}
                  </button>
                ))}
              </div>
            </div>
          )}

          {/* Stats bar */}
          {hasPoints && (
            <div style={{
              display: "flex", alignItems: "stretch",
              border: "1px solid var(--border)", borderRadius: 10,
              marginTop: 10, overflow: "hidden",
            }}>
              {([
                { label: "Points",            value: `${points.length} / ${totalRecords != null ? totalRecords.toLocaleString() : "—"}` },
                { label: "Explained Variance", value: `PC1 ${pct1.toFixed(1)}%  ·  PC2 ${pct2.toFixed(1)}%` },
                { label: "Avg. Distance",      value: avgDist.toFixed(3) },
                { label: "Clusters (HDBSCAN)", value: clusterCount > 0 ? String(clusterCount) : "—" },
              ] as const).map((stat, i, arr) => (
                <div key={stat.label} style={{
                  flex: 1, padding: "10px 14px",
                  borderRight: i < arr.length - 1 ? "1px solid var(--border)" : "none",
                }}>
                  <div style={{ fontSize: 11, color: "var(--muted-foreground)", marginBottom: 3 }}>
                    {stat.label}
                  </div>
                  <div style={{
                    fontSize: 13, fontWeight: 600,
                    color: "var(--foreground)", fontFamily: "ui-monospace, monospace",
                  }}>
                    {stat.value}
                  </div>
                </div>
              ))}
              <div style={{ padding: "10px 12px", display: "flex", alignItems: "center" }}>
                <Button
                  size="sm" variant="outline"
                  style={{ gap: 6, fontSize: 12, whiteSpace: "nowrap" }}
                  disabled
                  title="3D view coming soon"
                >
                  <Box size={13} />View in 3D
                </Button>
              </div>
            </div>
          )}
        </div>

        {/* Right panel */}
        {hasPoints && (
          <div style={{
            width: 222, flexShrink: 0,
            border: "1px solid var(--border)", borderRadius: 12, overflow: "hidden",
          }}>
            {/* Tabs */}
            <div style={{ display: "flex", borderBottom: "1px solid var(--border)" }}>
              {(["controls", "legend"] as const).map((tab) => (
                <button
                  key={tab}
                  onClick={() => setActiveTab(tab)}
                  style={{
                    flex: 1, padding: "9px 0", fontSize: 13,
                    fontWeight: activeTab === tab ? 500 : 400,
                    color: activeTab === tab ? "var(--foreground)" : "var(--muted-foreground)",
                    background: "transparent", border: "none",
                    borderBottom: activeTab === tab
                      ? "2px solid var(--v-accent)"
                      : "2px solid transparent",
                    cursor: "pointer", transition: "color 0.1s",
                  }}
                >
                  {tab === "controls" ? "Controls" : "Legend"}
                </button>
              ))}
            </div>

            {activeTab === "controls" && (
              <div style={{ padding: "14px", display: "flex", flexDirection: "column", gap: 14 }}>
                <ControlField label="Projection">
                  <select style={selectStyle}>
                    <option>PCA</option>
                  </select>
                </ControlField>

                <ControlField label="X-axis">
                  <select style={selectStyle}>
                    <option>PC1 ({pct1.toFixed(1)}%)</option>
                    <option>PC2 ({pct2.toFixed(1)}%)</option>
                  </select>
                </ControlField>

                <ControlField label="Y-axis">
                  <select style={selectStyle}>
                    <option>PC2 ({pct2.toFixed(1)}%)</option>
                    <option>PC1 ({pct1.toFixed(1)}%)</option>
                  </select>
                </ControlField>

                <ControlField label="Point style">
                  <div style={{
                    display: "flex", alignItems: "center", gap: 8,
                    border: "1px solid var(--border)", borderRadius: 6,
                    padding: "5px 8px", background: "var(--background)", cursor: "pointer",
                  }}>
                    <span style={{ width: 14, height: 14, borderRadius: "50%", background: "#818cf8", flexShrink: 0 }} />
                    <span style={{ fontSize: 12, color: "var(--muted-foreground)", flex: 1 }}>Indigo</span>
                    <span style={{ fontSize: 10, color: "var(--muted-foreground)" }}>▾</span>
                  </div>
                </ControlField>

                <ControlField label={`Point size  ${pointSize}`}>
                  <input
                    type="range" min={2} max={14} value={pointSize}
                    onChange={(e) => setPointSize(+e.target.value)}
                    style={rangeStyle}
                  />
                </ControlField>

                <ControlField label={`Opacity  ${opacity}%`}>
                  <input
                    type="range" min={10} max={100} value={opacity}
                    onChange={(e) => setOpacity(+e.target.value)}
                    style={rangeStyle}
                  />
                </ControlField>

                <div style={{ borderTop: "1px solid var(--border)", paddingTop: 14, display: "flex", flexDirection: "column", gap: 10 }}>
                  {([
                    { label: "Show vectors",        checked: true,        onChange: () => {} },
                    { label: "Show grid",           checked: showGrid,    onChange: setShowGrid },
                    { label: "Show labels",         checked: showLabels,  onChange: setShowLabels },
                    { label: "Show cluster colors", checked: showCluster, onChange: setShowCluster },
                  ] as const).map(({ label, checked, onChange }) => (
                    <div key={label} style={{ display: "flex", alignItems: "center", justifyContent: "space-between" }}>
                      <span style={{ fontSize: 12, color: "var(--foreground)" }}>{label}</span>
                      <Toggle checked={checked} onChange={onChange} />
                    </div>
                  ))}
                </div>

                {/* Info box */}
                <div style={{
                  background: "color-mix(in oklch, var(--v-accent) 8%, transparent)",
                  border: "1px solid color-mix(in oklch, var(--v-accent) 22%, transparent)",
                  borderRadius: 8, padding: "10px 12px",
                }}>
                  <div style={{ display: "flex", gap: 8, alignItems: "flex-start" }}>
                    <span style={{ fontSize: 13, lineHeight: 1, marginTop: 1 }}>ℹ</span>
                    <p style={{ fontSize: 11.5, color: "var(--muted-foreground)", lineHeight: 1.55, margin: 0 }}>
                      PCA reduces high-dimensional vectors to 2D while preserving as much variance as possible.
                    </p>
                  </div>
                </div>
              </div>
            )}

            {activeTab === "legend" && (
              <div style={{ padding: "14px" }}>
                <div style={{ fontSize: 12, fontWeight: 500, color: "var(--foreground)", marginBottom: 10 }}>
                  Clusters
                </div>
                {clusterCount === 0 ? (
                  <p style={{ fontSize: 12, color: "var(--muted-foreground)" }}>No clusters detected.</p>
                ) : (
                  Array.from({ length: clusterCount }, (_, i) => (
                    <div key={i} style={{ display: "flex", alignItems: "center", gap: 8, marginBottom: 8 }}>
                      <span style={{
                        width: 10, height: 10, borderRadius: "50%",
                        background: CLUSTER_COLORS[i % CLUSTER_COLORS.length],
                        display: "inline-block", flexShrink: 0,
                      }} />
                      <span style={{ fontSize: 12, color: "var(--muted-foreground)" }}>
                        Cluster {i + 1}
                        <span style={{ color: "var(--foreground)", marginLeft: 6, fontFamily: "ui-monospace, monospace" }}>
                          {points.filter((p) => p.cluster === i).length}
                        </span>
                      </span>
                    </div>
                  ))
                )}
                {points.filter((p) => p.cluster === -1).length > 0 && (
                  <div style={{ display: "flex", alignItems: "center", gap: 8, marginTop: 4 }}>
                    <span style={{
                      width: 10, height: 10, borderRadius: "50%",
                      background: "var(--muted-foreground)", display: "inline-block", flexShrink: 0,
                    }} />
                    <span style={{ fontSize: 12, color: "var(--muted-foreground)" }}>
                      Noise
                      <span style={{ color: "var(--foreground)", marginLeft: 6, fontFamily: "ui-monospace, monospace" }}>
                        {points.filter((p) => p.cluster === -1).length}
                      </span>
                    </span>
                  </div>
                )}
              </div>
            )}
          </div>
        )}
      </div>
    </div>
  );
}

// ─── Style constants ──────────────────────────────────────────────────────────

const menuItemStyle: React.CSSProperties = {
  display: "block", width: "100%", textAlign: "left",
  padding: "6px 10px", borderRadius: 6, border: "none",
  background: "transparent", color: "var(--foreground)",
  fontSize: 13, cursor: "pointer",
};

const iconBtnStyle: React.CSSProperties = {
  width: 30, height: 30, borderRadius: 6,
  border: "1px solid var(--border)",
  background: "var(--background)",
  color: "var(--muted-foreground)",
  display: "flex", alignItems: "center", justifyContent: "center",
  cursor: "pointer",
};

const zoomBtnStyle: React.CSSProperties = {
  width: 28, height: 28, borderRadius: 6,
  border: "1px solid rgba(255,255,255,0.14)",
  background: "rgba(0,0,0,0.40)",
  color: "rgba(255,255,255,0.65)",
  display: "flex", alignItems: "center", justifyContent: "center",
  cursor: "pointer", backdropFilter: "blur(4px)",
};

const selectStyle: React.CSSProperties = {
  width: "100%", padding: "5px 8px",
  borderRadius: 6, border: "1px solid var(--border)",
  background: "var(--background)", color: "var(--foreground)",
  fontSize: 12, cursor: "pointer",
};

const rangeStyle: React.CSSProperties = {
  width: "100%", accentColor: "var(--v-accent)",
};
