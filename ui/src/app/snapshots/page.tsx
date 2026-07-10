"use client";

import { useState, useEffect, useCallback, useRef } from "react";
import useSWR, { mutate as globalMutate } from "swr";
import { useProjectManifest } from "@/lib/hooks/useProjectManifest";

// -- Types ---------------------------------------------------------------------

interface SnapshotEntry {
  key: string;
  state_hash: string;
  epoch_secs: number;
  size_bytes: number;
}

interface LocalFile {
  name: string;
  path: string;
  kind: "snap" | "log" | "other";
  size_bytes: number;
  modified_at: string;
  exists: boolean;
}

interface Health {
  status: string;
  records: { live: number };
  event_log_height?: number;
  event_log_path?: string;
  snapshot_path?: string;
  dim?: number;
}

const LS_ENABLED    = "valori:auto-snap:enabled";
const LS_THRESHOLD  = "valori:auto-snap:threshold";
const LS_LAST_COUNT = "valori:auto-snap:last-count";
const LS_LAST_AT    = "valori:auto-snap:last-at";

// -- Helpers -------------------------------------------------------------------

const fetcher = (url: string) => fetch(url).then((r) => r.json());

function fmtBytes(b: number) {
  if (b < 1024) return `${b} B`;
  if (b < 1024 * 1024) return `${(b / 1024).toFixed(1)} KB`;
  return `${(b / 1024 / 1024).toFixed(2)} MB`;
}

function fmtCount(n: number) {
  if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(n % 1_000_000 === 0 ? 0 : 1)}M`;
  if (n >= 1_000) return `${(n / 1_000).toFixed(n % 1_000 === 0 ? 0 : 1)}k`;
  return String(n);
}

function fmtDate(epochSecs: number) {
  return new Date(epochSecs * 1000).toLocaleString(undefined, {
    month: "short", day: "numeric", year: "numeric",
    hour: "2-digit", minute: "2-digit",
  });
}

function fmtRelative(epochSecs: number) {
  const diff = Date.now() / 1000 - epochSecs;
  if (diff < 60) return "just now";
  if (diff < 3600) return `${Math.floor(diff / 60)}m ago`;
  if (diff < 86400) return `${Math.floor(diff / 3600)}h ago`;
  return `${Math.floor(diff / 86400)}d ago`;
}

// -- Copy button ---------------------------------------------------------------
function CopyBtn({ text }: { text: string }) {
  const [done, setDone] = useState(false);
  return (
    <button
      onClick={async (e) => {
        e.stopPropagation();
        await navigator.clipboard.writeText(text);
        setDone(true);
        setTimeout(() => setDone(false), 1400);
      }}
      className={`text-[10px] px-1.5 py-0.5 rounded border transition-all shrink-0 ${
        done
          ? "border-emerald-500 text-emerald-600 dark:text-emerald-400"
          : "border-input text-muted-foreground hover:text-accent-foreground hover:border-ring"
      }`}
    >
      {done ? "✓" : "copy"}
    </button>
  );
}

// -- Toast ---------------------------------------------------------------------
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

// -- Main page -----------------------------------------------------------------
export default function SnapshotsPage() {
  const [toast, setToast] = useState<{ msg: string; ok: boolean } | null>(null);
  const showToast = useCallback((msg: string, ok = true) => {
    setToast({ msg, ok });
    setTimeout(() => setToast(null), 4000);
  }, []);

  // Project switching
  const [selecting, setSelecting] = useState<string | null>(null);
  const { projects, open: openProject } = useProjectManifest();

  // Save actions
  const [localPath, setLocalPath] = useState("");
  const [savingLocal, setSavingLocal] = useState(false);
  const [savingS3, setSavingS3] = useState(false);

  // Restore
  const [restoringKey, setRestoringKey] = useState<string | null>(null);
  const [confirmKey, setConfirmKey] = useState<string | null>(null);

  // Auto-snapshot
  const [autoEnabled, setAutoEnabled] = useState(false);
  const [threshold, setThreshold] = useState(50_000);
  const [lastCount, setLastCount] = useState<number | null>(null);
  const [lastAt, setLastAt] = useState<string | null>(null);
  const autoTriggering = useRef(false);

  useEffect(() => {
    const en  = localStorage.getItem(LS_ENABLED) === "true";
    const thr = parseInt(localStorage.getItem(LS_THRESHOLD) ?? "50000", 10);
    const lc  = localStorage.getItem(LS_LAST_COUNT);
    const la  = localStorage.getItem(LS_LAST_AT);
    setAutoEnabled(en);
    if (!isNaN(thr)) setThreshold(thr);
    if (lc) setLastCount(parseInt(lc, 10));
    if (la) setLastAt(la);
  }, []);

  // Active connection → active project
  const { data: conn } = useSWR<{ url: string }>("/api/connection", fetcher, { revalidateOnFocus: false });
  const activePort = (() => {
    if (!conn?.url) return null;
    try { return parseInt(new URL(conn.url).port || "3000", 10); }
    catch { return null; }
  })();
  const activeProject = projects.find((p) => p.port === activePort) ?? null;

  async function handleSelectProject(name: string) {
    setSelecting(name);
    try {
      await openProject(name);
      await globalMutate(() => true);
    } finally {
      setSelecting(null);
    }
  }

  // Health
  const { data: health, mutate: mutateHealth } = useSWR<Health>(
    "/api/health", fetcher, { refreshInterval: 5000, revalidateOnFocus: false }
  );
  const eventCount = health?.event_log_height ?? null;

  // Auto-snapshot trigger
  useEffect(() => {
    if (!autoEnabled || eventCount === null) return;
    const base = lastCount ?? 0;
    if (eventCount < base + threshold) return;
    if (autoTriggering.current) return;
    autoTriggering.current = true;
    fetch("/api/storage/snapshots/upload", { method: "POST" })
      .then((r) => r.json())
      .then((d) => {
        const now = new Date().toISOString();
        setLastCount(eventCount);
        setLastAt(now);
        localStorage.setItem(LS_LAST_COUNT, String(eventCount));
        localStorage.setItem(LS_LAST_AT, now);
        showToast(`Auto-snapshot saved (${fmtBytes((d as { size_bytes?: number }).size_bytes ?? 0)})`, true);
        mutateSnaps();
      })
      .catch(() => showToast("Auto-snapshot failed", false))
      .finally(() => { autoTriggering.current = false; });
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [eventCount, autoEnabled, threshold, lastCount]);

  // Snapshots list
  const { data: snapsData, mutate: mutateSnaps } = useSWR<{
    snapshots: SnapshotEntry[];
    disabled?: boolean;
  }>("/api/storage/snapshots", fetcher, { refreshInterval: 15000 });
  const snapshots = [...(snapsData?.snapshots ?? [])].sort((a, b) => b.epoch_secs - a.epoch_secs);
  const objectStoreDisabled = snapsData?.disabled === true;

  // Local files
  const configuredPaths = [health?.event_log_path, health?.snapshot_path].filter(Boolean) as string[];
  const localFilesKey = configuredPaths.length > 0
    ? `/api/local-files?files=${encodeURIComponent(configuredPaths.join(","))}`
    : "/api/local-files";
  const { data: localData } = useSWR<{ files: LocalFile[] }>(localFilesKey, fetcher, { refreshInterval: 10000 });

  // Handlers
  async function handleLocalSave() {
    setSavingLocal(true);
    try {
      const body = localPath.trim() ? { path: localPath.trim() } : {};
      const res = await fetch("/api/snapshot/save", {
        method: "POST", headers: { "Content-Type": "application/json" },
        body: JSON.stringify(body),
      });
      const d = await res.json() as { path?: string; error?: string };
      if (!res.ok) throw new Error(d.error ?? `HTTP ${res.status}`);
      showToast(`Saved to ${d.path}`, true);
    } catch (e) {
      showToast(e instanceof Error ? e.message : "Save failed", false);
    } finally {
      setSavingLocal(false);
    }
  }

  async function handleS3Upload() {
    setSavingS3(true);
    try {
      const res = await fetch("/api/storage/snapshots/upload", { method: "POST" });
      const d = await res.json() as { key?: string; size_bytes?: number; error?: string };
      if (!res.ok) throw new Error(d.error ?? `HTTP ${res.status}`);
      showToast(`Saved to object store (${fmtBytes(d.size_bytes ?? 0)})`, true);
      mutateSnaps();
    } catch (e) {
      showToast(e instanceof Error ? e.message : "Upload failed", false);
    } finally {
      setSavingS3(false);
    }
  }

  async function handleRestore(key: string) {
    if (confirmKey !== key) {
      setConfirmKey(key);
      setTimeout(() => setConfirmKey(null), 5000);
      return;
    }
    setConfirmKey(null);
    setRestoringKey(key);
    try {
      const res = await fetch("/api/storage/snapshots/restore", {
        method: "POST", headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ key }),
      });
      const d = await res.json() as { state_hash?: string; error?: string };
      if (!res.ok) throw new Error(d.error ?? `HTTP ${res.status}`);
      showToast(`Restored — hash: ${d.state_hash?.slice(0, 12)}…`, true);
      mutateHealth();
    } catch (e) {
      showToast(e instanceof Error ? e.message : "Restore failed", false);
    } finally {
      setRestoringKey(null);
    }
  }

  function toggleAuto(enabled: boolean) {
    setAutoEnabled(enabled);
    localStorage.setItem(LS_ENABLED, String(enabled));
  }

  const progress = eventCount !== null && threshold > 0
    ? Math.min(((eventCount - (lastCount ?? 0)) / threshold) * 100, 100)
    : 0;

  const localFiles = (localData?.files ?? []).filter((f) => f.exists);

  return (
    <div className="flex flex-col gap-5 w-full max-w-[1600px]">

      {/* ── Page header ───────────────────────────────────────────────────── */}
      <div className="flex items-start justify-between gap-4">
        <div>
          <h1 className="text-lg font-semibold text-foreground">Snapshots</h1>
          <p className="text-xs text-muted-foreground mt-0.5">
            Point-in-time captures of the kernel state — save, download, or restore instantly.
          </p>
        </div>

        {/* Project picker */}
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

      {/* ── Status strip ──────────────────────────────────────────────────── */}
      <div className="flex items-center gap-3 px-4 py-2.5 rounded-xl border border-border bg-card">
        <StatusChip
          label="Records"
          value={health ? fmtCount(health.records?.live ?? 0) : "—"}
        />
        <div className="h-4 w-px bg-border" />
        <StatusChip
          label="Events committed"
          value={eventCount !== null ? fmtCount(eventCount) : "—"}
          hint={eventCount !== null ? String(eventCount) : undefined}
        />
        <div className="h-4 w-px bg-border" />
        <StatusChip
          label="Node"
          value={health?.status ?? "—"}
          ok={health?.status === "ok"}
        />
        {activeProject && (
          <>
            <div className="h-4 w-px bg-border" />
            <StatusChip label="Project" value={activeProject.name} />
          </>
        )}
        {!eventCount && health && (
          <span className="ml-auto text-[10px] text-muted-foreground">
            Set <code>VALORI_EVENT_LOG_PATH</code> for event count
          </span>
        )}
      </div>

      {/* ── Main two-column layout ────────────────────────────────────────── */}
      <div className="grid grid-cols-1 lg:grid-cols-[1fr_320px] gap-5">

        {/* LEFT — snapshot history */}
        <div className="flex flex-col gap-3 rounded-xl border border-border bg-card overflow-hidden">
          <div className="px-5 py-3 border-b border-border bg-background/50 flex items-center justify-between">
            <h2 className="text-xs font-semibold text-accent-foreground uppercase tracking-widest">
              History
              {snapshots.length > 0 && (
                <span className="ml-2 text-muted-foreground font-normal normal-case tracking-normal">
                  ({snapshots.length})
                </span>
              )}
            </h2>
            {objectStoreDisabled && (
              <span className="text-[10px] text-amber-500 bg-amber-500/10 border border-amber-500/25 px-2 py-0.5 rounded">
                object store not configured
              </span>
            )}
          </div>

          <div className="px-5 pb-4">
            {objectStoreDisabled ? (
              <div className="flex flex-col gap-2 py-6 items-center text-center">
                <p className="text-sm text-muted-foreground">No object store configured.</p>
                <p className="text-[11px] text-muted-foreground max-w-xs leading-relaxed">
                  Set <code className="text-muted-foreground">VALORI_OBJECT_STORE_URL</code> to{" "}
                  <code className="text-muted-foreground">s3://bucket/prefix</code> or{" "}
                  <code className="text-muted-foreground">file:///path</code> to enable cloud snapshots.
                </p>

                {/* Fall back to showing local files if object store is off */}
                {localFiles.length > 0 && (
                  <div className="mt-4 w-full text-left">
                    <p className="text-[10px] uppercase tracking-widest text-muted-foreground mb-2">Local files found</p>
                    <div className="flex flex-col divide-y divide-border/60 rounded-lg border border-border overflow-hidden">
                      {localFiles.map((f) => (
                        <div key={f.path} className="flex items-center gap-3 px-3 py-2.5">
                          <div className="flex-1 min-w-0">
                            <p className="font-mono text-[11px] text-accent-foreground truncate" title={f.name}>{f.name}</p>
                            <p className="font-mono text-[10px] text-muted-foreground/50 truncate" title={f.path}>{f.path}</p>
                          </div>
                          <CopyBtn text={f.path} />
                          <span className="text-[11px] text-muted-foreground tabular-nums shrink-0">
                            {fmtBytes(f.size_bytes)}
                          </span>
                        </div>
                      ))}
                    </div>
                  </div>
                )}
              </div>
            ) : snapshots.length === 0 ? (
              <div className="py-10 text-center">
                <p className="text-sm text-muted-foreground">No snapshots yet.</p>
                <p className="text-[11px] text-muted-foreground mt-1">
                  Use "Save to cloud" to create the first one.
                </p>
              </div>
            ) : (
              <div className="flex flex-col divide-y divide-border">
                {snapshots.map((snap) => (
                  <SnapshotRow
                    key={snap.key}
                    snap={snap}
                    confirming={confirmKey === snap.key}
                    restoring={restoringKey === snap.key}
                    onRestore={() => handleRestore(snap.key)}
                  />
                ))}
              </div>
            )}
          </div>
        </div>

        {/* RIGHT — capture actions + auto-snapshot */}
        <div className="flex flex-col gap-3">

          {/* Capture card */}
          <div className="rounded-xl border border-border bg-card overflow-hidden">
            <div className="px-5 py-3 border-b border-border bg-background/50">
              <h2 className="text-xs font-semibold text-accent-foreground uppercase tracking-widest">Capture</h2>
            </div>
            <div className="px-5 py-4 flex flex-col gap-4">

              {/* Save to cloud */}
              <div className="flex flex-col gap-2">
                <p className="text-xs font-medium text-foreground">Save to cloud</p>
                <p className="text-[11px] text-muted-foreground leading-relaxed">
                  Compressed, keyed by hash. Old snapshots are pruned automatically.
                  Requires <code className="text-muted-foreground">VALORI_OBJECT_STORE_URL</code>.
                </p>
                <button
                  onClick={handleS3Upload}
                  disabled={savingS3 || objectStoreDisabled}
                  className="mt-1 w-full rounded-lg border border-input bg-accent px-4 py-2 text-sm text-card-foreground hover:bg-muted disabled:opacity-40 transition-colors"
                >
                  {savingS3 ? "Saving…" : "↑ Save to object store"}
                </button>
              </div>

              <div className="border-t border-border" />

              {/* Save locally */}
              <div className="flex flex-col gap-2">
                <p className="text-xs font-medium text-foreground">Save locally</p>
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

              <div className="border-t border-border" />

              {/* Download */}
              <div className="flex flex-col gap-1.5">
                <p className="text-xs font-medium text-foreground">Download</p>
                <a
                  href="/api/snapshot/download"
                  download
                  className="mt-0.5 w-full text-center rounded-lg border border-input bg-accent px-4 py-1.5 text-xs text-card-foreground hover:bg-muted transition-colors"
                >
                  ↓ Download .snap
                </a>
              </div>
            </div>
          </div>

          {/* Auto-snapshot card */}
          <div className="rounded-xl border border-border bg-card overflow-hidden">
            <div className="px-5 py-3 border-b border-border bg-background/50 flex items-center justify-between">
              <h2 className="text-xs font-semibold text-accent-foreground uppercase tracking-widest">Auto-snapshot</h2>
              {/* Toggle */}
              <button
                onClick={() => toggleAuto(!autoEnabled)}
                className={`w-9 h-5 rounded-full relative transition-colors shrink-0 ${autoEnabled ? "bg-emerald-600" : "bg-muted"}`}
              >
                <span className={`absolute top-0.5 w-4 h-4 rounded-full bg-white transition-all ${autoEnabled ? "left-4" : "left-0.5"}`} />
              </button>
            </div>
            <div className="px-5 py-4 flex flex-col gap-3">
              {autoEnabled && objectStoreDisabled && (
                <div className="rounded-lg border border-amber-500/25 bg-amber-500/10 px-3 py-2 text-[11px] text-amber-500">
                  Object store not configured — auto-snapshots will fail.
                </div>
              )}

              <div className="flex flex-col gap-1.5">
                <p className="text-[11px] text-muted-foreground">Trigger every</p>
                <div className="flex gap-1.5 flex-wrap">
                  {[10_000, 50_000, 100_000, 500_000].map((t) => (
                    <button
                      key={t}
                      onClick={() => { setThreshold(t); localStorage.setItem(LS_THRESHOLD, String(t)); }}
                      className={`px-2.5 py-1 rounded-lg border text-[11px] transition-colors ${
                        threshold === t
                          ? "border-[var(--v-accent)] bg-[var(--v-accent-muted)] text-[var(--v-accent)]"
                          : "border-input text-muted-foreground hover:text-accent-foreground hover:border-ring"
                      }`}
                    >
                      {fmtCount(t)}
                    </button>
                  ))}
                </div>
                <p className="text-[10px] text-muted-foreground">events</p>
              </div>

              {autoEnabled && eventCount !== null && (
                <div className="flex flex-col gap-1.5 pt-1">
                  <div className="flex items-center justify-between text-[10px] text-muted-foreground">
                    <span>{fmtCount(Math.max(0, eventCount - (lastCount ?? 0)))} / {fmtCount(threshold)}</span>
                    <span>{Math.round(progress)}%</span>
                  </div>
                  <div className="h-1 rounded-full bg-accent overflow-hidden">
                    <div className="h-full rounded-full bg-emerald-600 transition-all duration-500" style={{ width: `${progress}%` }} />
                  </div>
                  {lastAt && (
                    <p className="text-[10px] text-muted-foreground">
                      Last: {new Date(lastAt).toLocaleString(undefined, { month: "short", day: "numeric", hour: "2-digit", minute: "2-digit" })}
                    </p>
                  )}
                </div>
              )}

              {!autoEnabled && (
                <p className="text-[11px] text-muted-foreground">
                  When enabled, the UI triggers a cloud snapshot whenever the event count crosses the threshold.
                </p>
              )}
            </div>
          </div>

          {/* Local files — collapsed by default */}
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
      </div>

      {toast && <Toast msg={toast.msg} ok={toast.ok} />}
    </div>
  );
}

// -- Sub-components ------------------------------------------------------------

function StatusChip({ label, value, hint, ok }: {
  label: string; value: string; hint?: string; ok?: boolean;
}) {
  return (
    <div className="flex items-center gap-2" title={hint}>
      <span className="text-[10px] text-muted-foreground uppercase tracking-widest shrink-0">{label}</span>
      <span className={`text-sm font-semibold tabular-nums ${
        ok === true  ? "text-emerald-500 dark:text-emerald-400" :
        ok === false ? "text-amber-500  dark:text-amber-400"   :
                       "text-foreground"
      }`}>
        {value}
      </span>
    </div>
  );
}

function SnapshotRow({ snap, confirming, restoring, onRestore }: {
  snap: SnapshotEntry;
  confirming: boolean;
  restoring: boolean;
  onRestore: () => void;
}) {
  const shortKey  = snap.key.split("/").pop() ?? snap.key;
  const shortHash = snap.state_hash.slice(0, 12);

  return (
    <div className="flex items-center gap-3 py-3">
      {/* Date + relative */}
      <div className="shrink-0 w-36">
        <p className="text-xs text-foreground tabular-nums">{fmtDate(snap.epoch_secs)}</p>
        <p className="text-[10px] text-muted-foreground">{fmtRelative(snap.epoch_secs)}</p>
      </div>

      {/* Hash + key */}
      <div className="flex-1 min-w-0">
        <div className="flex items-center gap-1.5">
          <span className="font-mono text-[10px] text-muted-foreground">{shortHash}…</span>
          <CopyBtn text={snap.state_hash} />
        </div>
        <p className="font-mono text-[10px] text-muted-foreground/50 truncate mt-0.5" title={snap.key}>
          {shortKey}
        </p>
      </div>

      {/* Size */}
      <span className="shrink-0 text-xs text-muted-foreground tabular-nums w-14 text-right">
        {fmtBytes(snap.size_bytes)}
      </span>

      {/* Restore */}
      <button
        onClick={onRestore}
        disabled={restoring}
        className={`shrink-0 text-[11px] px-2.5 py-1.5 rounded-lg border transition-all ${
          confirming
            ? "border-amber-500 bg-amber-500/10 text-amber-600 dark:text-amber-400 dark:border-amber-700 animate-pulse"
            : "border-input text-muted-foreground hover:border-ring hover:text-foreground"
        } disabled:opacity-40`}
      >
        {restoring ? "…" : confirming ? "confirm?" : "restore"}
      </button>
    </div>
  );
}
