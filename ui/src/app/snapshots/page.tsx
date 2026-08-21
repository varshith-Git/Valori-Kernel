"use client";

import { useState, useCallback } from "react";
import useSWR, { mutate as globalMutate } from "swr";
import { SnapshotsView } from "@valori/studio";
import { useProjectManifest } from "@/lib/hooks/useProjectManifest";
import { LocalFilesPanel } from "@/components/snapshots/LocalFilesPanel";
import { LOCAL_CONNECTION_PROJECT_ID } from "@/lib/local-runtime/transport";
import { resolveLocalCapabilities } from "@/lib/local-runtime/capabilities";

// Migrated to Shared Studio's SnapshotsView (Phase G2) — object-store
// history/save/restore/download and the Auto-snapshot card are now the
// same implementation every host consumes. The multi-project switcher
// stays entirely host-level (host navigation, not a Studio concern);
// "Save locally" + the local-files listing go through the
// `localFilesPanel` slot, gated by capabilities.localFilesystem — no
// /api/local-files inside Studio.
function Toast({ msg, ok }: { msg: string; ok: boolean }) {
  return (
    <div className={`fixed bottom-6 right-6 z-50 rounded-lg border px-4 py-3 text-sm shadow-xl ${
      ok
        ? "border-emerald-500/30 bg-emerald-500/15 text-emerald-600 dark:text-emerald-300"
        : "border-red-500/30 bg-red-500/15 text-red-600 dark:text-red-400"
    }`}>
      {msg}
    </div>
  );
}

export default function SnapshotsPage() {
  const [toast, setToast] = useState<{ msg: string; ok: boolean } | null>(null);
  const showToast = useCallback((msg: string, ok = true) => {
    setToast({ msg, ok });
    setTimeout(() => setToast(null), 4000);
  }, []);

  const [selecting, setSelecting] = useState<string | null>(null);
  const { projects, open: openProject } = useProjectManifest();

  // Which project is the browser's single active connection pointed at
  // right now — restored host-level state (Phase G3), never passed into
  // Shared Studio: SnapshotsView has no concept of "which project," it's
  // handed one already-resolved projectId, same as every other view.
  const { data: conn } = useSWR<{ url: string }>("/api/connection", (url: string) => fetch(url).then((r) => r.json()), { revalidateOnFocus: false });
  const activePort = (() => {
    if (!conn?.url) return null;
    try { return parseInt(new URL(conn.url).port || "3000", 10); }
    catch { return null; }
  })();

  async function handleSelectProject(name: string) {
    setSelecting(name);
    try {
      await openProject(name);
      await globalMutate(() => true);
    } finally {
      setSelecting(null);
    }
  }

  return (
    <div className="flex flex-col gap-5 w-full max-w-[1600px]">
      <div className="flex items-start justify-between gap-4">
        <div>
          <h1 className="text-lg font-semibold text-foreground">Snapshots</h1>
          <p className="text-xs text-muted-foreground mt-0.5">
            Point-in-time captures of the kernel state — save, download, or restore instantly.
          </p>
        </div>

        {projects.length > 0 && (
          <div className="flex items-center gap-2 shrink-0">
            <span className="text-xs text-muted-foreground">Project</span>
            <div className="flex gap-1.5 flex-wrap justify-end">
              {projects.map((p) => {
                const isActive = p.port === activePort;
                const isSel = selecting === p.name;
                return (
                  <button
                    key={p.name}
                    onClick={() => !isActive && handleSelectProject(p.name)}
                    disabled={isSel}
                    title={`Port :${p.port} · ${p.status}`}
                    className={`flex items-center gap-1.5 px-2.5 py-1 rounded-lg border text-xs transition-colors ${
                      isActive
                        ? "border-[var(--v-accent)] bg-[var(--v-accent-muted)] text-[var(--v-accent)] cursor-default"
                        : "border-input text-muted-foreground hover:text-accent-foreground hover:border-ring"
                    } disabled:opacity-60`}
                  >
                    <span className={`h-1.5 w-1.5 rounded-full shrink-0 ${
                      p.status === "running" ? "bg-emerald-400" :
                      p.status === "error"   ? "bg-red-400" : "bg-zinc-500"
                    }`} />
                    {p.name}
                    {isSel && <span className="opacity-60">…</span>}
                  </button>
                );
              })}
            </div>
          </div>
        )}
      </div>

      <SnapshotsView
        projectId={LOCAL_CONNECTION_PROJECT_ID}
        capabilities={resolveLocalCapabilities()}
        localFilesPanel={<LocalFilesPanel onToast={showToast} />}
      />

      {toast && <Toast msg={toast.msg} ok={toast.ok} />}
    </div>
  );
}
