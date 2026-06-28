"use client";

import { use, useEffect, useState, useCallback, useRef } from "react";
import { useRouter } from "next/navigation";
import { Loader2, AlertTriangle, ArrowLeft, Square, RotateCcw, RefreshCw } from "lucide-react";

/**
 * Wraps every `/projects/<name>/*` route. On mount (and whenever the project
 * changes) it ensures that project's node is running and points the UI proxy at
 * it — so deep links and hard refreshes auto-resume the right session. The open
 * call is idempotent: a no-op probe when the node is already up.
 */
export default function ProjectLayout({
  children,
  params,
}: {
  children: React.ReactNode;
  params: Promise<{ name: string }>;
}) {
  const { name } = use(params);
  const project = decodeURIComponent(name);
  const router = useRouter();

  const [state,      setState]      = useState<"opening" | "ready" | "failed">("opening");
  const [nodeStatus, setNodeStatus] = useState<"running" | "stopped" | "error">("running");
  const [actionBusy, setActionBusy] = useState(false);
  const [startLogs,  setStartLogs]  = useState<string[]>([]);
  const logsRef = useRef<HTMLDivElement>(null);

  const openProject = useCallback(async (): Promise<boolean> => {
    const r = await fetch(`/api/projects/${encodeURIComponent(project)}/open`, { method: "POST" });
    if (!r.ok) return false;
    const d = await r.json().catch(() => ({})) as { reachable?: boolean };
    return d.reachable !== false;
  }, [project]);

  // On mount — ensure the node is running.
  useEffect(() => {
    let cancelled = false;
    setState("opening");
    setStartLogs([]);
    openProject().then(ok => {
      if (cancelled) return;
      setState(ok ? "ready" : "failed");
      setNodeStatus(ok ? "running" : "error");
      if (!ok) {
        // Pull the last few startup log lines so the user can diagnose.
        fetch(`/api/launch/logs?nodeId=3010`)
          .then(r => r.text())
          .then(raw => {
            const lines = raw.split("\n")
              .filter(l => l.startsWith("data:"))
              .map(l => { try { return JSON.parse(l.slice(5).trim()) as string; } catch { return ""; } })
              .filter(Boolean)
              .slice(-20);
            if (!cancelled) setStartLogs(lines);
          }).catch(() => {});
      }
    }).catch(() => {
      if (!cancelled) { setState("failed"); setNodeStatus("error"); }
    });
    return () => { cancelled = true; };
  }, [project, openProject]);

  // Poll node status every 5 s so the bar stays accurate.
  useEffect(() => {
    if (state !== "ready") return;
    const id = setInterval(async () => {
      try {
        const r = await fetch(`/api/projects/${encodeURIComponent(project)}/open`, { method: "POST" });
        const d = await r.json().catch(() => ({})) as { reachable?: boolean };
        setNodeStatus(r.ok && d.reachable !== false ? "running" : "stopped");
      } catch {
        setNodeStatus("stopped");
      }
    }, 5000);
    return () => clearInterval(id);
  }, [state, project]);

  const handleStop = async () => {
    setActionBusy(true);
    await fetch(`/api/projects/${encodeURIComponent(project)}/close`, { method: "POST" });
    setNodeStatus("stopped");
    setActionBusy(false);
  };

  const handleRestart = async () => {
    setActionBusy(true);
    // Close first (no-op if already stopped), then re-open.
    await fetch(`/api/projects/${encodeURIComponent(project)}/close`, { method: "POST" });
    const ok = await openProject();
    setNodeStatus(ok ? "running" : "error");
    setActionBusy(false);
  };

  const handleStart = async () => {
    setActionBusy(true);
    const ok = await openProject();
    setNodeStatus(ok ? "running" : "error");
    if (ok && state === "failed") setState("ready");
    setActionBusy(false);
  };

  const handleRetry = async () => {
    setState("opening");
    setStartLogs([]);
    const ok = await openProject();
    setState(ok ? "ready" : "failed");
    setNodeStatus(ok ? "running" : "error");
  };

  if (state === "opening") {
    return (
      <div className="flex flex-col items-center justify-center gap-3 py-32 text-muted-foreground">
        <Loader2 size={22} className="animate-spin text-[var(--v-accent)]" />
        <p className="text-sm">Starting <span className="font-medium text-foreground">{project}</span>…</p>
        <p className="text-xs text-muted-foreground/70">Restoring snapshot &amp; replaying the audit log</p>
      </div>
    );
  }

  if (state === "failed") {
    return (
      <div className="flex flex-col gap-4 py-16 max-w-xl mx-auto">
        <div className="flex items-start gap-3">
          <AlertTriangle size={18} className="text-amber-500 mt-0.5 shrink-0" />
          <div>
            <p className="text-sm font-medium text-foreground">
              Couldn&apos;t start &quot;{project}&quot;
            </p>
            <p className="mt-1 text-xs text-muted-foreground">
              The node didn&apos;t respond in time. Your data is safe — the WAL is durable.
              Try retrying; if it fails again check the logs below.
            </p>
          </div>
        </div>

        <div className="flex items-center gap-2">
          <button
            onClick={handleRetry}
            className="flex items-center gap-1.5 rounded-lg bg-primary text-primary-foreground px-4 py-2 text-sm hover:bg-primary/90 transition-colors"
          >
            <RefreshCw size={13} /> Retry
          </button>
          <button
            onClick={() => router.push("/")}
            className="flex items-center gap-1.5 rounded-lg border border-border px-4 py-2 text-sm text-muted-foreground hover:text-foreground hover:bg-accent transition-colors"
          >
            <ArrowLeft size={13} /> Back to projects
          </button>
        </div>

        {startLogs.length > 0 && (
          <div className="rounded-lg border border-border bg-card overflow-hidden">
            <p className="text-[10px] uppercase tracking-widest text-muted-foreground px-3 py-2 border-b border-border">
              Startup logs
            </p>
            <div
              ref={logsRef}
              className="max-h-48 overflow-y-auto px-3 py-2 flex flex-col gap-0.5"
            >
              {startLogs.map((line, i) => (
                <code key={i} className="text-[11px] font-mono text-muted-foreground whitespace-pre-wrap break-all">
                  {line}
                </code>
              ))}
            </div>
          </div>
        )}

        <p className="text-xs text-muted-foreground">
          Common causes: port conflict, binary not yet compiled (run{" "}
          <code className="text-[10px] bg-muted px-1 rounded">cargo build -p valori-node --release</code>
          ), or a large WAL replay taking longer than 60 s.
        </p>
      </div>
    );
  }

  // ── Session control bar ───────────────────────────────────────────────────────
  const statusDot =
    nodeStatus === "running" ? "bg-emerald-400" :
    nodeStatus === "error"   ? "bg-red-400" :
                               "bg-zinc-500";
  const statusLabel =
    nodeStatus === "running" ? "running" :
    nodeStatus === "error"   ? "error" :
                               "stopped";

  return (
    <div className="flex flex-col gap-4">
      {/* Session bar */}
      <div className="flex items-center gap-3 rounded-xl border border-border bg-card px-4 py-2.5">
        {/* Status */}
        <span className={`h-2 w-2 rounded-full flex-shrink-0 ${statusDot} ${nodeStatus === "running" ? "animate-pulse" : ""}`} />
        <span className="text-xs text-muted-foreground font-mono">{project}</span>
        <span className={`text-[10px] font-mono px-1.5 py-0.5 rounded border ${
          nodeStatus === "running" ? "border-emerald-500/30 text-emerald-600 bg-emerald-500/10" :
          nodeStatus === "error"   ? "border-red-500/30 text-red-500 bg-red-500/10" :
                                     "border-border text-muted-foreground bg-accent"
        }`}>
          {statusLabel}
        </span>

        <div className="ml-auto flex items-center gap-2">
          {nodeStatus === "running" ? (
            <>
              {/* Restart */}
              <button
                onClick={handleRestart}
                disabled={actionBusy}
                title="Snapshot, stop, then restart"
                className="flex items-center gap-1.5 rounded-md border border-border bg-accent hover:bg-muted px-2.5 py-1 text-[11px] text-muted-foreground hover:text-foreground disabled:opacity-40 transition-colors"
              >
                {actionBusy ? <Loader2 size={11} className="animate-spin" /> : <RotateCcw size={11} />}
                Restart
              </button>
              {/* Stop */}
              <button
                onClick={handleStop}
                disabled={actionBusy}
                title="Snapshot & stop session"
                className="flex items-center gap-1.5 rounded-md border border-border bg-accent hover:bg-muted px-2.5 py-1 text-[11px] text-muted-foreground hover:text-foreground disabled:opacity-40 transition-colors"
              >
                {actionBusy ? <Loader2 size={11} className="animate-spin" /> : <Square size={11} />}
                Stop
              </button>
            </>
          ) : (
            /* Start */
            <button
              onClick={handleStart}
              disabled={actionBusy}
              title="Start session"
              className="flex items-center gap-1.5 rounded-md border border-emerald-500/40 bg-emerald-500/15 hover:bg-emerald-500/25 px-2.5 py-1 text-[11px] text-emerald-700 disabled:opacity-40 transition-colors"
            >
              {actionBusy ? <Loader2 size={11} className="animate-spin" /> : null}
              Start
            </button>
          )}

          {/* Back to projects */}
          <button
            onClick={() => router.push("/")}
            title="Back to all projects"
            className="flex items-center gap-1.5 rounded-md border border-border bg-accent hover:bg-muted px-2.5 py-1 text-[11px] text-muted-foreground hover:text-foreground transition-colors"
          >
            <ArrowLeft size={11} /> Projects
          </button>
        </div>
      </div>

      {children}
    </div>
  );
}
