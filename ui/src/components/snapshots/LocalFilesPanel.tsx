"use client";

// Local-only content for SnapshotsView's `localFilesPanel` slot (Phase G2).
// Deliberately NOT a Studio component — it calls Local's own /api/local-files
// and /api/snapshot/save routes directly, exactly the "supply local-file
// data through a host callback, don't put /api/local-files inside Studio"
// boundary the Phase G1 design set. Bundles two things that were separate
// cards on the old page (Save locally + the local-files listing) into one
// slot's content, since SnapshotsView only exposes one generic ReactNode
// slot, not a second capture-action card — composing them here avoids
// touching the shared component further for a single host's extra action.

import { useState } from "react";
import useSWR from "swr";
import { CopyBtn } from "@/components/ui/copy-btn";

interface LocalFile {
  name: string;
  path: string;
  kind: "snap" | "log" | "other";
  size_bytes: number;
  modified_at: string;
  exists: boolean;
}

interface Health {
  event_log_path?: string;
  snapshot_path?: string;
}

const fetcher = (url: string) => fetch(url).then((r) => r.json());

function fmtBytes(b: number) {
  if (b < 1024) return `${b} B`;
  if (b < 1024 * 1024) return `${(b / 1024).toFixed(1)} KB`;
  return `${(b / 1024 / 1024).toFixed(2)} MB`;
}

export function LocalFilesPanel({ onToast }: { onToast: (msg: string, ok?: boolean) => void }) {
  const [localPath, setLocalPath] = useState("");
  const [savingLocal, setSavingLocal] = useState(false);

  const { data: health } = useSWR<Health>("/api/health", fetcher, { revalidateOnFocus: false });
  const configuredPaths = [health?.event_log_path, health?.snapshot_path].filter(Boolean) as string[];
  const localFilesKey = configuredPaths.length > 0
    ? `/api/local-files?files=${encodeURIComponent(configuredPaths.join(","))}`
    : null;
  const { data: localData } = useSWR<{ files: LocalFile[] }>(localFilesKey, fetcher, { refreshInterval: 10000 });

  async function handleLocalSave() {
    setSavingLocal(true);
    try {
      const body = localPath.trim() ? { path: localPath.trim() } : {};
      const res = await fetch("/api/snapshot/save", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify(body),
      });
      const d = (await res.json()) as { path?: string; error?: string };
      if (!res.ok) throw new Error(d.error ?? `HTTP ${res.status}`);
      onToast(`Saved to ${d.path}`, true);
    } catch (e) {
      onToast(e instanceof Error ? e.message : "Save failed", false);
    } finally {
      setSavingLocal(false);
    }
  }

  return (
    <div className="flex flex-col gap-3">
      <div className="rounded-xl border border-border bg-card overflow-hidden">
        <div className="px-5 py-3 border-b border-border bg-background/50">
          <h2 className="text-xs font-semibold text-accent-foreground uppercase tracking-widest">Save locally</h2>
        </div>
        <div className="px-5 py-4 flex flex-col gap-2">
          <p className="text-[11px] text-muted-foreground">
            Leave blank to use <code className="text-muted-foreground">VALORI_SNAPSHOT_PATH</code>.
          </p>
          <div className="flex gap-2 mt-1">
            <input
              type="text"
              value={localPath}
              onChange={(e) => setLocalPath(e.target.value)}
              placeholder="optional path…"
              className="flex-1 min-w-0 rounded-lg border border-input bg-background px-3 py-1.5 text-xs text-card-foreground placeholder:text-muted-foreground focus:outline-none focus:ring-1 focus:ring-ring font-mono"
            />
            <button
              onClick={handleLocalSave}
              disabled={savingLocal}
              className="shrink-0 rounded-lg border border-input bg-accent px-3 py-1.5 text-xs text-card-foreground hover:bg-muted disabled:opacity-40 transition-colors"
            >
              {savingLocal ? "…" : "Save"}
            </button>
          </div>
        </div>
      </div>

      {configuredPaths.length > 0 && (
        <details className="rounded-xl border border-border bg-card overflow-hidden group">
          <summary className="px-5 py-3 cursor-pointer list-none flex items-center justify-between select-none">
            <span className="text-xs font-semibold text-accent-foreground uppercase tracking-widest">Local files</span>
            <span className="text-[10px] text-muted-foreground group-open:hidden">show</span>
            <span className="text-[10px] text-muted-foreground hidden group-open:inline">hide</span>
          </summary>
          <div className="px-5 pb-4 flex flex-col gap-2">
            {configuredPaths.map((p) => {
              const file = (localData?.files ?? []).find((f) => f.path === p);
              return (
                <div key={p} className="flex items-center gap-2">
                  <span className={`h-1.5 w-1.5 rounded-full shrink-0 ${file?.exists ? "bg-emerald-400" : "bg-zinc-500"}`} />
                  <span className="font-mono text-[11px] text-accent-foreground truncate flex-1 min-w-0">{p}</span>
                  <CopyBtn text={p} />
                  {file?.exists && (
                    <span className="text-[10px] text-muted-foreground shrink-0">{fmtBytes(file.size_bytes)}</span>
                  )}
                </div>
              );
            })}
          </div>
        </details>
      )}
    </div>
  );
}
