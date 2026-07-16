"use client";

import { useEffect, useState } from "react";

interface SnapshotEntry {
  key: string;
  size: number;
  last_modified: string;
}

interface SnapshotListResponse {
  snapshots: SnapshotEntry[];
  count: number;
  disabled?: boolean;
  error?: string;
}

function fmt(bytes: number) {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / 1024 / 1024).toFixed(2)} MB`;
}

function fmtDate(iso: string) {
  try { return new Date(iso).toLocaleString(); } catch { return iso; }
}

export default function SnapshotsPage() {
  const [data, setData] = useState<SnapshotListResponse | null>(null);
  const [loading, setLoading] = useState(true);
  const [uploading, setUploading] = useState(false);
  const [restoring, setRestoring] = useState<string | null>(null);
  const [msg, setMsg] = useState<{ ok: boolean; text: string } | null>(null);
  const [loadError, setLoadError] = useState(false);

  const load = async () => {
    setLoading(true);
    setLoadError(false);
    try {
      const res = await fetch("/api/storage/snapshots");
      const d: SnapshotListResponse = await res.json().catch(() => ({ snapshots: [], count: 0 }));
      setData(d);
    } catch {
      setLoadError(true);
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => { load(); }, []);

  const upload = async () => {
    setUploading(true);
    setMsg(null);
    try {
      const res = await fetch("/api/storage/snapshots/upload", { method: "POST" });
      const d = await res.json().catch(() => ({})) as { error?: string };
      if (res.ok) {
        setMsg({ ok: true, text: "Snapshot uploaded to object store" });
        await load();
      } else {
        setMsg({ ok: false, text: d.error ?? `Error ${res.status}` });
      }
    } catch {
      setMsg({ ok: false, text: "Upload failed" });
    } finally {
      setUploading(false);
    }
  };

  const restore = async (key: string) => {
    if (!confirm(`Restore snapshot ${key}? This will overwrite current state.`)) return;
    setRestoring(key);
    setMsg(null);
    try {
      const res = await fetch("/api/storage/snapshots/restore", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ key }),
      });
      const d = await res.json().catch(() => ({})) as { error?: string };
      if (res.ok) {
        setMsg({ ok: true, text: `Restored from ${key}` });
      } else {
        setMsg({ ok: false, text: d.error ?? `Error ${res.status}` });
      }
    } catch {
      setMsg({ ok: false, text: "Restore failed" });
    } finally {
      setRestoring(null);
    }
  };

  return (
    <div className="flex flex-col gap-6 w-full max-w-[1600px]">
      <div className="flex items-center justify-between">
        <div>
          <h1 className="text-xl font-semibold text-foreground">Snapshot Store</h1>
          <p className="mt-1 text-sm text-muted-foreground">
            S3 / MinIO / R2 snapshots. Each entry is a full deterministic state image.
          </p>
        </div>
        <button
          onClick={upload}
          disabled={uploading || data?.disabled}
          className="rounded-lg border border-input bg-card px-4 py-2 text-sm text-accent-foreground hover:bg-accent disabled:opacity-40 transition-colors"
        >
          {uploading ? "Uploading…" : "↑ Upload snapshot now"}
        </button>
      </div>

      {msg && (
        <div
          className={`rounded-lg border px-4 py-3 text-sm ${
            msg.ok
              ? "border-emerald-500/30 bg-emerald-500/12 text-emerald-400"
              : "border-red-500/30 bg-red-500/12 text-red-400"
          }`}
        >
          {msg.ok ? "✓" : "✗"} {msg.text}
        </div>
      )}

      {loading ? (
        <div className="flex flex-col gap-2 animate-pulse">
          {[1, 2, 3].map((i) => <div key={i} className="h-14 rounded-lg bg-accent" />)}
        </div>
      ) : loadError ? (
        <div className="rounded-xl border border-red-500/25 bg-red-500/10 p-6 flex flex-col gap-3">
          <p className="text-sm font-medium text-red-500">Couldn&apos;t reach the server</p>
          <button
            onClick={load}
            className="self-start rounded-lg border border-input bg-card px-3 py-1.5 text-xs text-accent-foreground hover:bg-accent transition-colors"
          >
            Retry
          </button>
        </div>
      ) : data?.disabled ? (
        <div className="rounded-xl border border-amber-500/25 bg-amber-500/12 p-6">
          <p className="text-sm font-medium text-amber-600 dark:text-amber-400">Object store not configured</p>
          <p className="text-xs text-amber-700 mt-2">
            Set <code className="font-mono bg-amber-500/20 px-1 rounded">VALORI_OBJECT_STORE_URL</code> to enable S3 snapshot storage.
          </p>
          <pre className="mt-3 rounded bg-background px-4 py-3 text-xs text-accent-foreground font-mono">
{`VALORI_OBJECT_STORE_URL=s3://my-bucket/valori
VALORI_OBJECT_STORE_REGION=us-east-1
# or for local MinIO:
VALORI_OBJECT_STORE_URL=s3://my-bucket/valori
VALORI_OBJECT_STORE_ENDPOINT=http://localhost:9000`}
          </pre>
        </div>
      ) : data?.snapshots?.length === 0 ? (
        <div className="rounded-xl border border-dashed border-border py-16 text-center">
          <p className="text-sm text-muted-foreground">No snapshots in object store yet.</p>
          <p className="text-xs text-muted-foreground mt-1">Click "Upload snapshot now" to push the first one.</p>
        </div>
      ) : (
        <div className="rounded-xl border border-border overflow-hidden">
          <div className="grid grid-cols-[1fr_8rem_12rem_8rem] gap-4 px-4 py-2.5 text-[10px] uppercase tracking-widest text-muted-foreground border-b border-border bg-card">
            <span>Key</span><span>Size</span><span>Modified</span><span></span>
          </div>
          {(data?.snapshots ?? []).map((s) => (
            <div
              key={s.key}
              className="grid grid-cols-[1fr_8rem_12rem_8rem] gap-4 items-center px-4 py-3 border-b border-border last:border-0 hover:bg-card/50 transition-colors"
            >
              <span className="font-mono text-xs text-accent-foreground truncate" title={s.key}>{s.key}</span>
              <span className="text-xs text-muted-foreground">{fmt(s.size)}</span>
              <span className="text-xs text-muted-foreground">{fmtDate(s.last_modified)}</span>
              <button
                onClick={() => restore(s.key)}
                disabled={restoring === s.key}
                className="text-xs text-muted-foreground hover:text-amber-600 dark:hover:text-amber-400 disabled:opacity-40 transition-colors text-right"
              >
                {restoring === s.key ? "restoring…" : "restore →"}
              </button>
            </div>
          ))}
        </div>
      )}

      {data && !data.disabled && (
        <div className="text-xs text-muted-foreground">
          {data.count} snapshot{data.count !== 1 ? "s" : ""} stored.
          Oldest snapshots pruned automatically per <code className="font-mono">VALORI_OBJECT_STORE_KEEP</code>.
        </div>
      )}
    </div>
  );
}
