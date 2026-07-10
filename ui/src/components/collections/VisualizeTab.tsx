"use client";

import { useState, useRef, useEffect, useCallback } from "react";
import { Button } from "@/components/ui/button";

interface Point {
  id: number;
  x: number;
  y: number;
  score: number;
}

// Power-iteration PCA: project N x D matrix to N x 2.
function pca2d(vecs: number[][]): [number[], number[]] {
  const n = vecs.length;
  const d = vecs[0]?.length ?? 0;
  if (n === 0 || d === 0) return [[], []];

  // Center
  const mean = Array(d).fill(0) as number[];
  for (const v of vecs) for (let j = 0; j < d; j++) mean[j] += v[j] / n;
  const centered = vecs.map((v) => v.map((x, j) => x - mean[j]));

  // Power iteration for top-2 eigenvectors of X^T X
  function powerIter(data: number[][], exclude?: number[]): number[] {
    let u: number[] = Array(d).fill(0).map((_, i) => (i === 0 ? 1 : 0));
    for (let iter = 0; iter < 30; iter++) {
      // v = X * X^T * u  projected (compute X^T * (X * u))
      const Xu = data.map((row) => row.reduce((s, x, j) => s + x * u[j], 0));
      let next = Array(d).fill(0) as number[];
      for (let i = 0; i < data.length; i++) {
        for (let j = 0; j < d; j++) next[j] += Xu[i] * data[i][j];
      }
      // Deflate if excluding a previous eigenvector
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
  return [proj1, proj2];
}

interface Props {
  namespace: string;
  dim: number | null;
}

const BATCH_SIZE = 20;

/** Fetch record vectors in sequential batches to avoid saturating the connection pool. */
async function fetchVectorsBatched(
  ids: number[],
  qs: string,
  onProgress: (done: number, total: number) => void,
): Promise<({ id: number; vector: number[] } | null)[]> {
  const results: ({ id: number; vector: number[] } | null)[] = [];
  for (let i = 0; i < ids.length; i += BATCH_SIZE) {
    const batch = ids.slice(i, i + BATCH_SIZE);
    const batchResults = await Promise.all(
      batch.map((id) =>
        fetch(`/api/records/${id}${qs}`)
          .then((r) => r.json() as Promise<{ id: number; vector: number[] }>)
          .catch(() => null),
      ),
    );
    results.push(...batchResults);
    onProgress(Math.min(i + BATCH_SIZE, ids.length), ids.length);
  }
  return results;
}

export function VisualizeTab({ namespace, dim }: Props) {
  const [points, setPoints] = useState<Point[]>([]);
  const [loading, setLoading] = useState(false);
  const [progress, setProgress] = useState<{ done: number; total: number } | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [hovered, setHovered] = useState<Point | null>(null);
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const MAX_POINTS = 200; // reduced: 500 was impractical without a batch endpoint

  const load = useCallback(async () => {
    if (dim == null) { setError("Vector dimension not known yet"); return; }
    setLoading(true);
    setProgress(null);
    setError(null);
    try {
      const res = await fetch("/api/search", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ query: Array(dim).fill(0), k: MAX_POINTS, collection: namespace }),
      });
      if (!res.ok) throw new Error(`Search failed (${res.status})`);
      const data = await res.json() as { results: { id: number; score: number }[] };

      const results = data.results ?? [];
      const qs = namespace ? `?collection=${encodeURIComponent(namespace)}` : "";

      // Fetch vectors in batches of BATCH_SIZE to avoid saturating the connection pool
      const fetched = await fetchVectorsBatched(
        results.map((r) => r.id),
        qs,
        (done, total) => setProgress({ done, total }),
      );
      setProgress(null);
      const vecs = fetched.filter(Boolean) as { id: number; vector: number[] }[];
      if (vecs.length < 2) { setError("Need at least 2 records to visualize"); return; }

      const [proj1, proj2] = pca2d(vecs.map((v) => v.vector));
      const pts: Point[] = vecs.map((v, i) => ({
        id: v.id,
        x: proj1[i],
        y: proj2[i],
        score: results.find((r) => r.id === v.id)?.score ?? 0,
      }));
      setPoints(pts);
    } catch (e) {
      setProgress(null);
      setError(e instanceof Error ? e.message : "Failed to load");
    } finally {
      setLoading(false);
    }
  }, [dim, namespace]);

  // Draw canvas
  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas || points.length === 0) return;
    const ctx = canvas.getContext("2d");
    if (!ctx) return;

    const W = canvas.width;
    const H = canvas.height;
    const pad = 32;

    const xs = points.map((p) => p.x);
    const ys = points.map((p) => p.y);
    const minX = Math.min(...xs), maxX = Math.max(...xs);
    const minY = Math.min(...ys), maxY = Math.max(...ys);
    const rangeX = maxX - minX || 1;
    const rangeY = maxY - minY || 1;

    const toCanvasX = (x: number) => pad + ((x - minX) / rangeX) * (W - pad * 2);
    const toCanvasY = (y: number) => H - pad - ((y - minY) / rangeY) * (H - pad * 2);

    // Clear
    const bg = getComputedStyle(canvas).getPropertyValue("--vis-bg").trim() || "#18181b";
    ctx.fillStyle = bg;
    ctx.fillRect(0, 0, W, H);

    // Grid lines
    ctx.strokeStyle = getComputedStyle(canvas).getPropertyValue("--vis-grid").trim() || "rgba(128,128,128,0.12)";
    ctx.lineWidth = 1;
    for (let i = 0; i <= 4; i++) {
      const gx = pad + (i / 4) * (W - pad * 2);
      const gy = pad + (i / 4) * (H - pad * 2);
      ctx.beginPath(); ctx.moveTo(gx, pad); ctx.lineTo(gx, H - pad); ctx.stroke();
      ctx.beginPath(); ctx.moveTo(pad, gy); ctx.lineTo(W - pad, gy); ctx.stroke();
    }

    // Dots
    const maxScore = Math.max(...points.map((p) => p.score)) || 1;
    for (const p of points) {
      const cx = toCanvasX(p.x);
      const cy = toCanvasY(p.y);
      const t = 1 - p.score / maxScore; // higher score = worse match, so invert
      // Color: blue-violet gradient by closeness
      const r = Math.round(99 + (139 - 99) * (1 - t));
      const g = Math.round(102 + (92 - 102) * (1 - t));
      const b = Math.round(241);
      ctx.beginPath();
      ctx.arc(cx, cy, 4, 0, Math.PI * 2);
      ctx.fillStyle = hovered?.id === p.id ? "#f59e0b" : `rgba(${r},${g},${b},0.75)`;
      ctx.fill();
    }
  }, [points, hovered]);

  const handleMouseMove = useCallback((e: React.MouseEvent<HTMLCanvasElement>) => {
    const canvas = canvasRef.current;
    if (!canvas || points.length === 0) return;
    const rect = canvas.getBoundingClientRect();
    const mx = (e.clientX - rect.left) * (canvas.width / rect.width);
    const my = (e.clientY - rect.top) * (canvas.height / rect.height);
    const pad = 32;
    const W = canvas.width, H = canvas.height;

    const xs = points.map((p) => p.x);
    const ys = points.map((p) => p.y);
    const minX = Math.min(...xs), maxX = Math.max(...xs);
    const minY = Math.min(...ys), maxY = Math.max(...ys);
    const rangeX = maxX - minX || 1;
    const rangeY = maxY - minY || 1;

    const toCanvasX = (x: number) => pad + ((x - minX) / rangeX) * (W - pad * 2);
    const toCanvasY = (y: number) => H - pad - ((y - minY) / rangeY) * (H - pad * 2);

    let closest: Point | null = null;
    let minDist = 12;
    for (const p of points) {
      const dist = Math.hypot(toCanvasX(p.x) - mx, toCanvasY(p.y) - my);
      if (dist < minDist) { minDist = dist; closest = p; }
    }
    setHovered(closest);
  }, [points]);

  return (
    <div className="flex flex-col gap-4">
      <div className="flex items-center justify-between">
        <div>
          <p className="text-sm text-muted-foreground">
            2D PCA projection of up to {MAX_POINTS} vectors. Hover a dot to see the record ID.
          </p>
        </div>
        <Button size="sm" onClick={load} disabled={loading || dim == null}>
          {loading ? (progress ? `${progress.done}/${progress.total}…` : "Searching…") : points.length > 0 ? "Reload" : "Load"}
        </Button>
      </div>

      {error && (
        <div className="rounded-lg border border-red-800/50 bg-red-950/20 px-4 py-3 text-sm text-red-400">
          {error}
        </div>
      )}

      {points.length === 0 && !loading && !error && (
        <div className="rounded-xl border border-dashed border-border py-16 text-center">
          <p className="text-sm text-muted-foreground">Click Load to fetch vectors and render the scatter plot.</p>
        </div>
      )}

      {points.length > 0 && (
        <div className="relative rounded-xl border border-border overflow-hidden bg-[var(--vis-bg)]">
          <canvas
            ref={canvasRef}
            width={800}
            height={480}
            className="w-full"
            onMouseMove={handleMouseMove}
            onMouseLeave={() => setHovered(null)}
            style={{ cursor: hovered ? "crosshair" : "default" }}
          />
          {hovered && (
            <div className="absolute bottom-3 left-3 rounded-lg border border-border bg-card/90 backdrop-blur-sm px-3 py-1.5 text-xs font-mono text-foreground">
              #{hovered.id} · score {hovered.score.toFixed(4)}
            </div>
          )}
          <div className="absolute top-3 right-3 text-[10px] font-mono text-muted-foreground/60">
            {points.length} points · PC1 × PC2
          </div>
        </div>
      )}
    </div>
  );
}
