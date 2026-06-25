"use client";

import { useState, useEffect, useCallback, useRef } from "react";
import useSWR from "swr";

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

// -- Auto-snapshot localStorage keys ------------------------------------------
const LS_ENABLED   = "valori:auto-snap:enabled";
const LS_THRESHOLD = "valori:auto-snap:threshold";
const LS_LAST_COUNT = "valori:auto-snap:last-count";   // event count at last trigger
const LS_LAST_AT   = "valori:auto-snap:last-at";       // ISO timestamp of last trigger

// -- Helpers -------------------------------------------------------------------

const fetcher = (url: string) =>
  fetch(url).then((r) => r.json());

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

// -- Copy button ---------------------------------------------------------------
function CopyBtn({ text }: { text: string }) {
  const [done, setDone] = useState(false);
  return (
    <button
      onClick={async () => {
        await navigator.clipboard.writeText(text);
        setDone(true);
        setTimeout(() => setDone(false), 1400);
      }}
      className={`text-[10px] px-1.5 py-0.5 rounded border transition-all ${
        done
          ? "border-emerald-700 text-emerald-400"
          : "border-input text-muted-foreground hover:text-accent-foreground hover:border-ring"
      }`}
    >
      {done ? "✓" : "copy"}
    </button>
  );
}

// -- Section card --------------------------------------------------------------
function Card({ title, children }: { title: string; children: React.ReactNode }) {
  return (
    <div className="rounded-xl border border-border bg-card overflow-hidden">
      <div className="px-5 py-3 border-b border-border bg-background/50">
        <h2 className="text-xs font-semibold text-accent-foreground uppercase tracking-widest">{title}</h2>
      </div>
      <div className="px-5 py-4">{children}</div>
    </div>
  );
}

// -- Toast ---------------------------------------------------------------------
function Toast({ msg, ok }: { msg: string; ok: boolean }) {
  return (
    <div
      className={`fixed bottom-6 right-6 z-50 rounded-lg border px-4 py-3 text-sm shadow-xl ${
        ok
          ? "border-emerald-500/30 bg-emerald-500/15 text-emerald-700"
          : "border-red-500/30 bg-red-500/15 text-red-700"
      }`}
    >
      {msg}
    </div>
  );
}

// -- Main page -----------------------------------------------------------------
export default function SnapshotsPage() {
  // -- State ------------------------------------------------------------------
  const [toast, setToast] = useState<{ msg: string; ok: boolean } | null>(null);
  const showToast = useCallback((msg: string, ok = true) => {
    setToast({ msg, ok });
    setTimeout(() => setToast(null), 4000);
  }, []);

  // Local save
  const [localPath, setLocalPath] = useState("");
  const [savingLocal, setSavingLocal] = useState(false);

  // S3 upload
  const [savingS3, setSavingS3] = useState(false);

  // Restore
  const [restoringKey, setRestoringKey] = useState<string | null>(null);
  const [confirmKey, setConfirmKey] = useState<string | null>(null);

  // Auto-snapshot settings (from localStorage)
  const [autoEnabled, setAutoEnabled] = useState(false);
  const [threshold, setThreshold] = useState(50_000);
  const [customThreshold, setCustomThreshold] = useState("");
  const [useCustom, setUseCustom] = useState(false);
  const [lastCount, setLastCount] = useState<number | null>(null);
  const [lastAt, setLastAt] = useState<string | null>(null);
  const autoTriggering = useRef(false);

  // -- Load persisted auto-snapshot settings ----------------------------------
  useEffect(() => {
    const en = localStorage.getItem(LS_ENABLED) === "true";
    const thr = parseInt(localStorage.getItem(LS_THRESHOLD) ?? "50000", 10);
    const lc = localStorage.getItem(LS_LAST_COUNT);
    const la = localStorage.getItem(LS_LAST_AT);
    setAutoEnabled(en);
    if (!isNaN(thr)) setThreshold(thr);
    if (lc) setLastCount(parseInt(lc, 10));
    if (la) setLastAt(la);
  }, []);

  // -- Health poll (every 5 s) ------------------------------------------------
  const { data: health, mutate: mutateHealth } = useSWR<Health>(
    "/api/health",
    fetcher,
    { refreshInterval: 5000, revalidateOnFocus: false }
  );

  const eventCount = health?.event_log_height ?? null;
  const effectiveThreshold = useCustom
    ? parseInt(customThreshold, 10) || threshold
    : threshold;

  // -- Auto-snapshot trigger --------------------------------------------------
  useEffect(() => {
    if (!autoEnabled || eventCount === null) return;
    const base = lastCount ?? 0;
    if (eventCount < base + effectiveThreshold) return;
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
        showToast(
          `Auto-snapshot saved (${fmtBytes((d as { size_bytes?: number }).size_bytes ?? 0)})`,
          true
        );
        mutateSnaps();
      })
      .catch(() => showToast("Auto-snapshot failed", false))
      .finally(() => { autoTriggering.current = false; });
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [eventCount, autoEnabled, effectiveThreshold, lastCount]);

  // -- Snapshot list (object store) ------------------------------------------
  const { data: snapsData, mutate: mutateSnaps } = useSWR<{
    snapshots: SnapshotEntry[];
    disabled?: boolean;
  }>("/api/storage/snapshots", fetcher, { refreshInterval: 15000 });

  // -- Local files ------------------------------------------------------------
  const [extraDir, setExtraDir] = useState("");
  const [extraDirInput, setExtraDirInput] = useState("");

  // Build the local-files URL:
  // - If health gave us specific configured paths, stat exactly those files
  //   (avoids scanning all of /tmp and picking up unrelated files)
  // - If user added an extra directory, scan that directory too
  const configuredPaths = [health?.event_log_path, health?.snapshot_path]
    .filter(Boolean) as string[];

  const localFilesKey = (() => {
    const params = new URLSearchParams();
    if (configuredPaths.length > 0) {
      // Stat configured files; also include any extra dir scan
      params.set("files", configuredPaths.join(","));
    }
    if (extraDir) {
      // Extra dir added by user: scan that directory for any .snap/.log
      const url = `/api/local-files?dirs=${encodeURIComponent(extraDir)}`;
      return url;
    }
    return configuredPaths.length > 0
      ? `/api/local-files?${params.toString()}`
      : "/api/local-files"; // fallback: reads env vars server-side
  })();

  const { data: localData, mutate: mutateLocal } = useSWR<{
    files: LocalFile[];
    scanned: string[];
  }>(localFilesKey, fetcher, { refreshInterval: 10000 });

  const snapshots: SnapshotEntry[] = snapsData?.snapshots ?? [];
  const objectStoreDisabled = snapsData?.disabled === true;

  // -- Handlers --------------------------------------------------------------

  async function handleLocalSave() {
    setSavingLocal(true);
    try {
      const body = localPath.trim() ? { path: localPath.trim() } : {};
      const res = await fetch("/api/snapshot/save", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
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
      const d = await res.json() as { key?: string; size_bytes?: number; state_hash?: string; error?: string };
      if (!res.ok) throw new Error(d.error ?? `HTTP ${res.status}`);
      showToast(`Uploaded: ${d.key} (${fmtBytes(d.size_bytes ?? 0)})`, true);
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
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ key }),
      });
      const d = await res.json() as { state_hash?: string; error?: string };
      if (!res.ok) throw new Error(d.error ?? `HTTP ${res.status}`);
      showToast(`Restored. State hash: ${d.state_hash?.slice(0, 16)}…`, true);
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

  function applyThreshold(t: number) {
    setThreshold(t);
    setUseCustom(false);
    localStorage.setItem(LS_THRESHOLD, String(t));
  }

  function applyCustomThreshold() {
    const t = parseInt(customThreshold, 10);
    if (isNaN(t) || t <= 0) return;
    setThreshold(t);
    setUseCustom(true);
    localStorage.setItem(LS_THRESHOLD, String(t));
  }

  const nextAt = (lastCount ?? 0) + effectiveThreshold;
  const progress = eventCount !== null && effectiveThreshold > 0
    ? Math.min(((eventCount - (lastCount ?? 0)) / effectiveThreshold) * 100, 100)
    : 0;

  return (
    <div className="flex flex-col gap-6 max-w-3xl">
      <div>
        <h1 className="text-lg font-semibold text-foreground">Snapshots</h1>
        <p className="text-xs text-muted-foreground mt-0.5">
          Point-in-time captures of the full kernel state. Restore to any saved snapshot instantly.
        </p>
      </div>

      {/* -- Current state -- */}
      <Card title="Current state">
        <div className="grid grid-cols-3 gap-4">
          <Stat
            label="Records"
            value={health ? String(health.records?.live ?? "N/A") : "—"}
          />
          <Stat
            label="Events committed"
            value={eventCount !== null ? fmtCount(eventCount) : "—"}
            sub={eventCount !== null ? String(eventCount) : undefined}
          />
          <Stat
            label="Server status"
            value={health?.status ?? "—"}
            ok={health?.status === "ok"}
          />
        </div>
        {!eventCount && (
          <p className="mt-3 text-[11px] text-muted-foreground">
            Event count requires <code className="text-muted-foreground">VALORI_EVENT_LOG_PATH</code> to be set.
          </p>
        )}
      </Card>

      {/* -- Manual snapshot -- */}
      <Card title="Take snapshot now">
        <div className="flex flex-col gap-5">
          {/* Object store (S3 / local path) */}
          <div className="flex flex-col gap-2">
            <p className="text-xs text-muted-foreground font-medium">Object store (S3 / file://)</p>
            <p className="text-[11px] text-muted-foreground">
              Requires <code className="text-muted-foreground">VALORI_OBJECT_STORE_URL</code>.
              Snapshot is compressed, keyed by timestamp + state hash, and old snapshots are pruned automatically.
            </p>
            <div className="flex gap-2 mt-1">
              <button
                onClick={handleS3Upload}
                disabled={savingS3 || objectStoreDisabled}
                className="rounded-lg border border-input bg-accent px-4 py-2 text-sm text-card-foreground hover:bg-muted disabled:opacity-40 transition-colors"
              >
                {savingS3 ? "Saving…" : "↑ Save to object store"}
              </button>
              {objectStoreDisabled && (
                <span className="self-center text-[11px] text-amber-600">
                  Object store not configured
                </span>
              )}
            </div>
          </div>

          <div className="border-t border-border" />

          {/* Local file save */}
          <div className="flex flex-col gap-2">
            <p className="text-xs text-muted-foreground font-medium">Local file save</p>
            <p className="text-[11px] text-muted-foreground">
              Saves to a path on the server filesystem. Leave blank to use{" "}
              <code className="text-muted-foreground">VALORI_SNAPSHOT_PATH</code>.
            </p>
            <div className="flex gap-2 mt-1">
              <input
                type="text"
                value={localPath}
                onChange={(e) => setLocalPath(e.target.value)}
                placeholder="/tmp/valori-snapshot.snap  (optional)"
                className="flex-1 rounded-lg border border-input bg-background px-3 py-2 text-sm text-card-foreground placeholder:text-muted-foreground focus:outline-none focus:ring-1 focus:ring-ring font-mono"
              />
              <button
                onClick={handleLocalSave}
                disabled={savingLocal}
                className="rounded-lg border border-input bg-accent px-4 py-2 text-sm text-card-foreground hover:bg-muted disabled:opacity-40 transition-colors whitespace-nowrap"
              >
                {savingLocal ? "Saving…" : "Save locally"}
              </button>
            </div>
          </div>

          <div className="border-t border-border" />

          {/* Download binary */}
          <div className="flex flex-col gap-2">
            <p className="text-xs text-muted-foreground font-medium">Download snapshot binary</p>
            <p className="text-[11px] text-muted-foreground">
              Downloads the current state as a <code className="text-muted-foreground">.snap</code> file directly to your browser.
            </p>
            <a
              href="/api/snapshot/download"
              download
              className="inline-flex w-fit rounded-lg border border-input bg-accent px-4 py-2 text-sm text-card-foreground hover:bg-muted transition-colors"
            >
              ↓ Download .snap
            </a>
          </div>
        </div>
      </Card>

      {/* -- Auto-snapshot -- */}
      <Card title="Auto-snapshot">
        <div className="flex flex-col gap-4">
          {/* Enable toggle */}
          <div className="flex items-center justify-between">
            <div>
              <p className="text-sm text-accent-foreground">Automatic snapshots</p>
              <p className="text-[11px] text-muted-foreground mt-0.5">
                The UI monitors the event count and triggers a snapshot when the threshold is reached.
                Saves to the configured object store.
              </p>
            </div>
            <button
              onClick={() => toggleAuto(!autoEnabled)}
              className={`flex-shrink-0 ml-4 w-10 h-5 rounded-full relative transition-colors ${
                autoEnabled ? "bg-emerald-700" : "bg-muted"
              }`}
            >
              <span
                className={`absolute top-0.5 w-4 h-4 rounded-full bg-white transition-all ${
                  autoEnabled ? "left-5" : "left-0.5"
                }`}
              />
            </button>
          </div>

          {autoEnabled && objectStoreDisabled && (
            <div className="rounded-lg border border-amber-500/25 bg-amber-500/10 px-3 py-2 text-xs text-amber-500">
              Object store not configured — auto-snapshots will fail.
              Set <code>VALORI_OBJECT_STORE_URL</code> and restart the server.
            </div>
          )}

          {/* Threshold selector */}
          <div className="flex flex-col gap-2">
            <p className="text-xs text-muted-foreground">Snapshot every N events</p>
            <div className="flex gap-2 flex-wrap">
              {[10_000, 50_000, 100_000, 500_000, 1_000_000].map((t) => (
                <button
                  key={t}
                  onClick={() => applyThreshold(t)}
                  className={`px-3 py-1.5 rounded-lg border text-xs transition-colors ${
                    threshold === t && !useCustom
                      ? "border-sky-700 bg-sky-950/50 text-sky-300"
                      : "border-input text-muted-foreground hover:text-accent-foreground hover:border-ring"
                  }`}
                >
                  {fmtCount(t)}
                </button>
              ))}

              {/* Custom */}
              <div className="flex gap-1">
                <input
                  type="number"
                  value={customThreshold}
                  onChange={(e) => setCustomThreshold(e.target.value)}
                  placeholder="custom…"
                  className="w-28 rounded-lg border border-input bg-background px-2 py-1.5 text-xs text-card-foreground placeholder:text-muted-foreground focus:outline-none focus:ring-1 focus:ring-ring"
                />
                <button
                  onClick={applyCustomThreshold}
                  disabled={!customThreshold}
                  className="px-3 py-1.5 rounded-lg border border-input text-xs text-muted-foreground hover:text-accent-foreground hover:border-ring disabled:opacity-40 transition-colors"
                >
                  set
                </button>
              </div>
            </div>

            {useCustom && (
              <p className="text-[11px] text-sky-600">
                Custom threshold: every {fmtCount(effectiveThreshold)} events
              </p>
            )}
          </div>

          {/* Progress bar */}
          {autoEnabled && eventCount !== null && (
            <div className="flex flex-col gap-1.5">
              <div className="flex items-center justify-between text-[11px]">
                <span className="text-muted-foreground">
                  {fmtCount(Math.max(0, eventCount - (lastCount ?? 0)))} / {fmtCount(effectiveThreshold)} events since last snapshot
                </span>
                <span className="text-muted-foreground">
                  Next at {fmtCount(nextAt)} events
                </span>
              </div>
              <div className="h-1.5 rounded-full bg-accent overflow-hidden">
                <div
                  className="h-full rounded-full bg-sky-600 transition-all duration-500"
                  style={{ width: `${progress}%` }}
                />
              </div>
              {lastAt && (
                <p className="text-[10px] text-muted-foreground">
                  Last triggered: {new Date(lastAt).toLocaleString(undefined, {
                    month: "short", day: "numeric", hour: "2-digit", minute: "2-digit",
                  })}
                </p>
              )}
            </div>
          )}

          {/* Server-side env guide */}
          <details className="mt-1">
            <summary className="text-[11px] text-muted-foreground cursor-pointer hover:text-muted-foreground transition-colors">
              Also configure server-side auto-snapshots (env vars)
            </summary>
            <div className="mt-2 rounded-lg bg-background border border-border px-4 py-3 font-mono text-[11px] text-muted-foreground space-y-1">
              <div className="flex items-center gap-2">
                <span className="text-muted-foreground">VALORI_SNAPSHOT_EVERY_EVENTS</span>
                <span>=</span>
                <span className="text-emerald-600">50000</span>
                <span className="text-muted-foreground font-sans"># trigger after N events</span>
              </div>
              <div className="flex items-center gap-2">
                <span className="text-muted-foreground">VALORI_SNAPSHOT_EVERY_BYTES</span>
                <span>=</span>
                <span className="text-emerald-600">67108864</span>
                <span className="text-muted-foreground font-sans"># trigger after 64 MB of WAL</span>
              </div>
              <div className="flex items-center gap-2">
                <span className="text-muted-foreground">VALORI_SNAPSHOT_KEEP</span>
                <span>=</span>
                <span className="text-emerald-600">5</span>
                <span className="text-muted-foreground font-sans"># keep N local snapshots</span>
              </div>
              <div className="flex items-center gap-2">
                <span className="text-muted-foreground">VALORI_OBJECT_STORE_URL</span>
                <span>=</span>
                <span className="text-emerald-600">s3://my-bucket/valori</span>
                <span className="text-muted-foreground font-sans"># or file:///local/path</span>
              </div>
              <div className="flex items-center gap-2">
                <span className="text-muted-foreground">VALORI_OBJECT_STORE_KEEP</span>
                <span>=</span>
                <span className="text-emerald-600">7</span>
                <span className="text-muted-foreground font-sans"># keep N remote snapshots</span>
              </div>
              <div className="flex items-center gap-2">
                <span className="text-muted-foreground">VALORI_OBJECT_STORE_REGION</span>
                <span>=</span>
                <span className="text-emerald-600">us-east-1</span>
              </div>
            </div>
          </details>
        </div>
      </Card>

      {/* -- Snapshot list -- */}
      <Card title={`Saved snapshots${snapshots.length > 0 ? ` (${snapshots.length})` : ""}`}>
        {objectStoreDisabled ? (
          <div className="flex flex-col gap-3">
            <p className="text-sm text-muted-foreground">Object store not configured.</p>
            <p className="text-[11px] text-muted-foreground leading-relaxed">
              Set <code className="text-muted-foreground">VALORI_OBJECT_STORE_URL</code> to{" "}
              <code className="text-muted-foreground">s3://bucket/prefix</code> or{" "}
              <code className="text-muted-foreground">file:///path/to/dir</code> and restart the server.
              Snapshots will be listed and restorable from here.
            </p>
          </div>
        ) : snapshots.length === 0 ? (
          <p className="text-sm text-muted-foreground">
            No snapshots yet. Click "Save to object store" above to create the first one.
          </p>
        ) : (
          <div className="flex flex-col divide-y divide-border">
            {[...snapshots]
              .sort((a, b) => b.epoch_secs - a.epoch_secs)
              .map((snap) => (
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
      </Card>

      {/* -- Local files -- */}
      <LocalFilesCard
        files={localData?.files ?? []}
        configuredPaths={configuredPaths}
        extraDir={extraDir}
        extraDirInput={extraDirInput}
        onExtraDirInputChange={setExtraDirInput}
        onAddDir={() => {
          const d = extraDirInput.trim();
          if (d) { setExtraDir(d); setExtraDirInput(""); }
        }}
        onRemoveDir={() => { setExtraDir(""); setExtraDirInput(""); }}
        onRefresh={() => mutateLocal()}
      />

      {toast && <Toast msg={toast.msg} ok={toast.ok} />}
    </div>
  );
}

// -- Sub-components ------------------------------------------------------------

function Stat({
  label,
  value,
  sub,
  ok,
}: {
  label: string;
  value: string;
  sub?: string;
  ok?: boolean;
}) {
  return (
    <div className="flex flex-col gap-0.5">
      <p className="text-[10px] text-muted-foreground uppercase tracking-widest">{label}</p>
      <p
        className={`text-lg font-semibold tabular-nums ${
          ok === true ? "text-emerald-400" : ok === false ? "text-amber-400" : "text-foreground"
        }`}
      >
        {value}
      </p>
      {sub && <p className="text-[10px] text-muted-foreground font-mono">{sub}</p>}
    </div>
  );
}

// -- Local Files card ----------------------------------------------------------
function LocalFilesCard({
  files,
  configuredPaths,
  extraDir,
  extraDirInput,
  onExtraDirInputChange,
  onAddDir,
  onRemoveDir,
  onRefresh,
}: {
  files: LocalFile[];
  configuredPaths: string[];
  extraDir: string;
  extraDirInput: string;
  onExtraDirInputChange: (v: string) => void;
  onAddDir: () => void;
  onRemoveDir: () => void;
  onRefresh: () => void;
}) {
  const snaps = files.filter((f) => f.kind === "snap");
  const logs  = files.filter((f) => f.kind === "log");
  const existingCount = files.filter((f) => f.exists).length;

  const noConfig = configuredPaths.length === 0 && !extraDir;

  return (
    <Card title={`Configured files — ${existingCount} of ${files.length} exist`}>
      {/* How files are being resolved */}
      <div className="flex flex-col gap-2 mb-4">
        {configuredPaths.length > 0 ? (
          <div className="rounded-lg border border-border bg-background px-3 py-2.5 space-y-1.5">
            <p className="text-[11px] text-muted-foreground font-medium mb-1">From server configuration (VALORI_* env vars)</p>
            {configuredPaths.map((p) => (
              <div key={p} className="flex items-center gap-2">
                <span className="font-mono text-[11px] text-accent-foreground">{p}</span>
                <CopyBtn text={p} />
              </div>
            ))}
          </div>
        ) : (
          <div className="rounded-lg border border-amber-500/25 bg-amber-500/10 px-3 py-2.5">
            <p className="text-xs text-amber-500">
              Server has no paths configured. Set <code className="font-mono">VALORI_EVENT_LOG_PATH</code> and{" "}
              <code className="font-mono">VALORI_SNAPSHOT_PATH</code> when starting the server.
            </p>
          </div>
        )}

        {/* Refresh + extra dir scan */}
        <div className="flex gap-2 mt-1">
          <input
            type="text"
            value={extraDirInput}
            onChange={(e) => onExtraDirInputChange(e.target.value)}
            onKeyDown={(e) => e.key === "Enter" && onAddDir()}
            placeholder="Scan extra directory (e.g. /var/lib/valori)"
            className="flex-1 rounded-lg border border-input bg-background px-3 py-1.5 text-[12px] font-mono text-card-foreground placeholder:text-muted-foreground focus:outline-none focus:ring-1 focus:ring-ring"
          />
          <button
            onClick={onAddDir}
            disabled={!extraDirInput.trim()}
            className="rounded-lg border border-input bg-accent px-3 py-1.5 text-xs text-muted-foreground hover:text-card-foreground hover:bg-muted disabled:opacity-40 transition-colors whitespace-nowrap"
          >
            + scan dir
          </button>
          <button
            onClick={onRefresh}
            className="rounded-lg border border-input bg-accent px-3 py-1.5 text-xs text-muted-foreground hover:text-card-foreground hover:bg-muted transition-colors"
            title="Refresh"
          >
            ↻
          </button>
          {extraDir && (
            <button
              onClick={onRemoveDir}
              className="rounded-lg border border-red-500/25 bg-red-500/10 px-3 py-1.5 text-xs text-red-500 hover:bg-red-950/40 transition-colors"
              title="Stop scanning extra directory"
            >
              ✕ {extraDir}
            </button>
          )}
        </div>
      </div>

      {noConfig && files.length === 0 ? (
        <p className="text-sm text-muted-foreground">No configured paths. Add server env vars or scan a directory above.</p>
      ) : files.length === 0 ? (
        <p className="text-sm text-muted-foreground">No .snap or .log files found.</p>
      ) : (
        <div className="flex flex-col gap-4">
          {logs.length > 0 && (
            <FileGroup label="Event log" color="amber" files={logs} />
          )}
          {snaps.length > 0 && (
            <FileGroup label="Snapshots" color="sky" files={snaps} />
          )}
        </div>
      )}
    </Card>
  );
}

function FileGroup({
  label,
  color,
  files,
}: {
  label: string;
  color: "sky" | "amber";
  files: LocalFile[];
}) {
  const existing = files.filter((f) => f.exists);
  const total = existing.reduce((s, f) => s + f.size_bytes, 0);
  const colorCls = color === "sky" ? "text-sky-400 border-sky-900/50 bg-sky-950/20" : "text-amber-400 border-amber-500/25 bg-amber-500/10";
  const dotCls = color === "sky" ? "bg-sky-500" : "bg-amber-500";

  return (
    <div>
      <div className="flex items-center gap-2 mb-2">
        <span className={`w-1.5 h-1.5 rounded-full flex-shrink-0 ${dotCls}`} />
        <span className="text-[11px] text-muted-foreground font-medium uppercase tracking-widest">
          {label}
        </span>
        <span className="text-[10px] text-muted-foreground">
          {existing.length} of {files.length} file{files.length !== 1 ? "s" : ""} exist · {fmtBytes(total)}
        </span>
      </div>
      <div className="flex flex-col divide-y divide-border/60 rounded-lg border border-border overflow-hidden">
        {files.map((f) => (
          <div
            key={f.path}
            className={`flex items-center gap-3 px-3 py-2.5 ${f.exists ? colorCls : "text-muted-foreground/80 border-border/50 bg-accent/20"}`}
          >
            {/* Filename + path */}
            <div className="flex-1 min-w-0">
              <span className={`font-mono text-[12px] ${f.exists ? "text-accent-foreground" : "text-muted-foreground/80"}`} title={f.path}>
                {f.name}
              </span>
              <p className="text-[10px] text-muted-foreground/50 font-mono truncate">{f.path}</p>
            </div>

            {/* Path copy */}
            <CopyBtn text={f.path} />

            {/* Size / not-yet badge */}
            {f.exists ? (
              <>
                <span className="text-[11px] text-muted-foreground tabular-nums w-16 text-right flex-shrink-0">
                  {fmtBytes(f.size_bytes)}
                </span>
                <span className="text-[10px] text-muted-foreground w-32 text-right flex-shrink-0 tabular-nums">
                  {new Date(f.modified_at).toLocaleString(undefined, {
                    month: "short", day: "numeric",
                    hour: "2-digit", minute: "2-digit",
                  })}
                </span>
              </>
            ) : (
              <span className="text-[11px] text-muted-foreground/80 w-48 text-right flex-shrink-0">
                not created yet
              </span>
            )}
          </div>
        ))}
      </div>
    </div>
  );
}

function SnapshotRow({
  snap,
  confirming,
  restoring,
  onRestore,
}: {
  snap: SnapshotEntry;
  confirming: boolean;
  restoring: boolean;
  onRestore: () => void;
}) {
  const shortKey = snap.key.split("/").pop() ?? snap.key;
  const shortHash = snap.state_hash.slice(0, 16);

  return (
    <div className="flex items-center gap-4 py-3 group">
      {/* Timestamp */}
      <div className="flex-shrink-0 w-40">
        <p className="text-xs text-accent-foreground tabular-nums">{fmtDate(snap.epoch_secs)}</p>
      </div>

      {/* Key + hash */}
      <div className="flex-1 min-w-0">
        <p className="text-[11px] font-mono text-muted-foreground truncate" title={snap.key}>
          {shortKey}
        </p>
        <div className="flex items-center gap-1.5 mt-0.5">
          <span className="text-[10px] font-mono text-muted-foreground">{shortHash}…</span>
          <CopyBtn text={snap.state_hash} />
        </div>
      </div>

      {/* Size */}
      <div className="flex-shrink-0 text-xs text-muted-foreground tabular-nums w-16 text-right">
        {fmtBytes(snap.size_bytes)}
      </div>

      {/* Restore button */}
      <div className="flex-shrink-0">
        <button
          onClick={onRestore}
          disabled={restoring}
          title={confirming ? "Click again to confirm restore" : "Restore this snapshot"}
          className={`text-[11px] px-3 py-1.5 rounded-lg border transition-all ${
            confirming
              ? "border-amber-700 bg-amber-950/50 text-amber-700 animate-pulse"
              : "border-input text-muted-foreground hover:border-ring hover:text-card-foreground"
          } disabled:opacity-40`}
        >
          {restoring ? "restoring…" : confirming ? "confirm?" : "restore"}
        </button>
      </div>
    </div>
  );
}
