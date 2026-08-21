"use client";

/**
 * IndexLifecycleTab — Phase 4.2
 *
 * Renders the live collection index lifecycle state and exposes
 * Create / Change / Remove actions. Works in both standalone and
 * cluster mode. Phase 4.3 added full ANN index support in cluster
 * mode — the previous 501 limitation is gone.
 *
 * States handled:
 *   none     → no ANN index; exact search active; "Create Index" offered
 *   building → active gen still serves; replacement gen building
 *   ready    → build finished, activation imminent (auto-activates server-side)
 *   active   → ANN index serving; "Change" and "Remove" offered
 *   failed   → last build failed; previous active (if any) still serves; "Retry" offered
 */

import { useState, useEffect } from "react";
import { TabShell } from "@/components/collections/TabShell";
import { useCollectionIndex, type IndexStatusResponse } from "@/lib/hooks/useCollectionIndex";
import { useTransport } from "@/runtime/context";
import type { ProjectRef } from "@/runtime/project";
import { cn } from "@/lib/utils";

// ── Constants ─────────────────────────────────────────────────────────────────

const INDEX_TYPES = ["hnsw", "ivf", "bq"] as const;
type IndexType = (typeof INDEX_TYPES)[number];

const DISPLAY_NAMES: Record<string, string> = {
  hnsw: "HNSW",
  ivf:  "IVF",
  bq:   "BQ (Binary Quantization)",
};

// Default parameter hints shown in the form.
const HNSW_DEFAULTS = { m: 16, ef_construction: 200, ef_search: 50 };
const IVF_DEFAULTS  = { n_list: "auto", n_probe: "auto" };

// ── Small helper components ───────────────────────────────────────────────────

function StatusDot({ status }: { status: string }) {
  const cls =
    status === "active"   ? "bg-emerald-500" :
    status === "building" ? "bg-amber-400 animate-pulse" :
    status === "ready"    ? "bg-sky-400 animate-pulse" :
    status === "failed"   ? "bg-red-500" :
    "bg-muted-foreground/40";
  return <span className={cn("inline-block w-2 h-2 rounded-full shrink-0", cls)} />;
}

function StatusBadge({ status }: { status: string }) {
  const label =
    status === "active"   ? "Active" :
    status === "building" ? "Building…" :
    status === "ready"    ? "Activating…" :
    status === "failed"   ? "Failed" :
    "None";
  const cls =
    status === "active"   ? "bg-emerald-500/10 text-emerald-700 dark:text-emerald-400 border-emerald-500/20" :
    status === "building" ? "bg-amber-500/10 text-amber-700 dark:text-amber-400 border-amber-500/20" :
    status === "ready"    ? "bg-sky-500/10 text-sky-700 dark:text-sky-400 border-sky-500/20" :
    status === "failed"   ? "bg-red-500/10 text-red-700 dark:text-red-400 border-red-500/20" :
    "bg-muted text-muted-foreground border-border";
  return (
    <span className={cn("inline-flex items-center gap-1.5 text-xs font-medium px-2 py-0.5 rounded-full border", cls)}>
      <StatusDot status={status} />
      {label}
    </span>
  );
}

function InfoRow({ label, value, className }: { label: string; value: React.ReactNode; className?: string }) {
  return (
    <div className={cn("flex items-center justify-between px-4 py-3 border-b border-border last:border-0", className)}>
      <span className="text-sm text-muted-foreground">{label}</span>
      <span className="text-sm font-medium text-foreground">{value}</span>
    </div>
  );
}

// ── Build form shared between Create and Change ───────────────────────────────

interface BuildFormProps {
  title: string;
  submitLabel: string;
  warning?: string;
  onSubmit: (type: IndexType, params: Record<string, unknown>) => void;
  onCancel: () => void;
  pending: boolean;
  error: string | null;
}

function BuildForm({ title, submitLabel, warning, onSubmit, onCancel, pending, error }: BuildFormProps) {
  const [indexType, setIndexType] = useState<IndexType>("hnsw");
  const [m, setM] = useState(String(HNSW_DEFAULTS.m));
  const [efC, setEfC] = useState(String(HNSW_DEFAULTS.ef_construction));
  const [efS, setEfS] = useState(String(HNSW_DEFAULTS.ef_search));
  const [nList, setNList] = useState(String(IVF_DEFAULTS.n_list));
  const [nProbe, setNProbe] = useState(String(IVF_DEFAULTS.n_probe));

  const handleSubmit = (e: React.FormEvent) => {
    e.preventDefault();
    const params: Record<string, unknown> = {};
    if (indexType === "hnsw") {
      const mv = parseInt(m, 10);
      const efCv = parseInt(efC, 10);
      const efSv = parseInt(efS, 10);
      if (!isNaN(mv))   params.m = mv;
      if (!isNaN(efCv)) params.ef_construction = efCv;
      if (!isNaN(efSv)) params.ef_search = efSv;
    } else if (indexType === "ivf") {
      const nlv = parseInt(nList, 10);
      const npv = parseInt(nProbe, 10);
      if (!isNaN(nlv)) params.n_list = nlv;
      if (!isNaN(npv)) params.n_probe = npv;
    }
    onSubmit(indexType, params);
  };

  const inputCls = "w-24 px-2 py-1 text-sm rounded-md border border-input bg-background text-foreground focus:outline-none focus:ring-1 focus:ring-ring";

  return (
    <div className="rounded-xl border border-border bg-card p-4 flex flex-col gap-3">
      <p className="text-sm font-medium text-foreground">{title}</p>

      {warning && (
        <div className="rounded-lg bg-amber-500/10 border border-amber-500/20 px-3 py-2 text-xs text-amber-700 dark:text-amber-400">
          {warning}
        </div>
      )}

      <form onSubmit={handleSubmit} className="flex flex-col gap-3">
        {/* Index type selector */}
        <div className="flex items-center gap-3">
          <span className="text-sm text-muted-foreground w-24 shrink-0">Index type</span>
          <select
            value={indexType}
            onChange={(e) => setIndexType(e.target.value as IndexType)}
            className="flex-1 px-2 py-1 text-sm rounded-md border border-input bg-background text-foreground focus:outline-none focus:ring-1 focus:ring-ring"
          >
            {INDEX_TYPES.map((t) => (
              <option key={t} value={t}>{DISPLAY_NAMES[t]}</option>
            ))}
          </select>
        </div>

        {/* HNSW parameters */}
        {indexType === "hnsw" && (
          <div className="flex flex-col gap-2">
            <p className="text-xs text-muted-foreground">
              Parameters — leave blank for node defaults (m=16, ef_construction=200, ef_search=50)
            </p>
            <div className="flex items-center gap-4 flex-wrap">
              <label className="flex items-center gap-2 text-xs">
                <span className="text-muted-foreground w-20">m</span>
                <input value={m} onChange={(e) => setM(e.target.value)} placeholder="16" className={inputCls} />
              </label>
              <label className="flex items-center gap-2 text-xs">
                <span className="text-muted-foreground w-20">ef_construction</span>
                <input value={efC} onChange={(e) => setEfC(e.target.value)} placeholder="200" className={inputCls} />
              </label>
              <label className="flex items-center gap-2 text-xs">
                <span className="text-muted-foreground w-20">ef_search</span>
                <input value={efS} onChange={(e) => setEfS(e.target.value)} placeholder="50" className={inputCls} />
              </label>
            </div>
          </div>
        )}

        {/* IVF parameters */}
        {indexType === "ivf" && (
          <div className="flex flex-col gap-2">
            <p className="text-xs text-muted-foreground">
              Parameters — leave blank for auto-scaling (n_list=sqrt(N), n_probe=sqrt(n_list))
            </p>
            <div className="flex items-center gap-4 flex-wrap">
              <label className="flex items-center gap-2 text-xs">
                <span className="text-muted-foreground w-16">n_list</span>
                <input value={nList} onChange={(e) => setNList(e.target.value)} placeholder="auto" className={inputCls} />
              </label>
              <label className="flex items-center gap-2 text-xs">
                <span className="text-muted-foreground w-16">n_probe</span>
                <input value={nProbe} onChange={(e) => setNProbe(e.target.value)} placeholder="auto" className={inputCls} />
              </label>
            </div>
          </div>
        )}

        {/* BQ — no parameters */}
        {indexType === "bq" && (
          <p className="text-xs text-muted-foreground">
            Binary Quantization compresses vectors to 1-bit per dimension. No build parameters.
            Artifact persistence is not supported for BQ — it rebuilds on restart.
          </p>
        )}

        {error && (
          <div className="rounded-lg bg-red-500/10 border border-red-500/20 px-3 py-2 text-xs text-red-700 dark:text-red-400">
            {error}
          </div>
        )}

        <div className="flex items-center gap-2 pt-1">
          <button
            type="submit"
            disabled={pending}
            className="px-3 py-1.5 text-xs font-medium rounded-lg bg-primary text-primary-foreground hover:bg-primary/80 disabled:opacity-50 transition-colors"
          >
            {pending ? "Starting build…" : submitLabel}
          </button>
          <button
            type="button"
            onClick={onCancel}
            className="px-3 py-1.5 text-xs font-medium rounded-lg border border-border text-muted-foreground hover:text-foreground hover:bg-accent transition-colors"
          >
            Cancel
          </button>
        </div>
      </form>
    </div>
  );
}

// ── Remove confirmation inline panel ─────────────────────────────────────────

function RemovePanel({
  activeType,
  onConfirm,
  onCancel,
  pending,
  error,
}: {
  activeType: string;
  onConfirm: () => void;
  onCancel: () => void;
  pending: boolean;
  error: string | null;
}) {
  return (
    <div className="rounded-xl border border-border bg-card p-4 flex flex-col gap-3">
      <p className="text-sm font-medium text-foreground">Remove {DISPLAY_NAMES[activeType] ?? activeType.toUpperCase()} index?</p>
      <p className="text-xs text-muted-foreground">
        This collection will revert to exact namespace-scoped search.
        The {DISPLAY_NAMES[activeType] ?? activeType.toUpperCase()} index will be retired and its artifact deleted.
        Vectors and graph data are unaffected.
      </p>
      {error && (
        <div className="rounded-lg bg-red-500/10 border border-red-500/20 px-3 py-2 text-xs text-red-700 dark:text-red-400">
          {error}
        </div>
      )}
      <div className="flex items-center gap-2">
        <button
          onClick={onConfirm}
          disabled={pending}
          className="px-3 py-1.5 text-xs font-medium rounded-lg bg-destructive/10 text-destructive hover:bg-destructive/20 disabled:opacity-50 transition-colors border border-destructive/20"
        >
          {pending ? "Removing…" : "Remove index"}
        </button>
        <button
          onClick={onCancel}
          className="px-3 py-1.5 text-xs font-medium rounded-lg border border-border text-muted-foreground hover:text-foreground hover:bg-accent transition-colors"
        >
          Cancel
        </button>
      </div>
    </div>
  );
}

// ── Main tab component ────────────────────────────────────────────────────────

type PanelState = "idle" | "create" | "change" | "remove";

export function IndexLifecycleTab({
  projectId,
  namespace,
}: {
  projectId: ProjectRef;
  namespace: string;
}) {
  const transport = useTransport();
  const { data, isLoading, error: fetchError, mutate } = useCollectionIndex(projectId, namespace);

  const [panel, setPanel] = useState<PanelState>("idle");
  const [actionPending, setActionPending] = useState(false);
  const [actionError, setActionError] = useState<string | null>(null);

  // Reset panel when namespace changes.
  useEffect(() => {
    setPanel("idle");
    setActionError(null);
  }, [namespace]);

  // POST helper — fires the index lifecycle endpoint.
  const postIndex = async (payload: Record<string, unknown>): Promise<string | null> => {
    const url = transport.path(projectId, `/namespaces/${encodeURIComponent(namespace)}/index`);
    const res = await fetch(url, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify(payload),
    });
    if (!res.ok) {
      const body = await res.json().catch(() => ({})) as { error?: string };
      return body.error ?? `Server error (${res.status})`;
    }
    return null;
  };

  const handleBuild = async (type: IndexType, params: Record<string, unknown>) => {
    setActionPending(true);
    setActionError(null);
    const payload: Record<string, unknown> = { type };
    if (Object.keys(params).length > 0) payload.parameters = params;
    const err = await postIndex(payload);
    setActionPending(false);
    if (err) {
      setActionError(err);
    } else {
      setPanel("idle");
      mutate();
    }
  };

  const handleRemove = async () => {
    setActionPending(true);
    setActionError(null);
    const err = await postIndex({ type: null });
    setActionPending(false);
    if (err) {
      setActionError(err);
    } else {
      setPanel("idle");
      mutate();
    }
  };

  // ── Loading ──────────────────────────────────────────────────────────────────
  if (isLoading) {
    return (
      <TabShell>
        <div className="animate-pulse space-y-2">
          <div className="h-5 w-24 rounded bg-accent" />
          <div className="h-20 rounded-xl bg-accent" />
        </div>
      </TabShell>
    );
  }

  if (fetchError) {
    return (
      <TabShell>
        <div className="rounded-xl border border-border bg-card p-4 text-sm text-destructive">
          Could not load index status: {fetchError.message}
        </div>
      </TabShell>
    );
  }

  const status      = data?.status ?? "none";
  const activeType  = data?.active_type ?? "none";
  const activeGen   = data?.active_generation;
  const buildingGen = data?.building_generation;
  const desiredType = data?.desired_type;
  const buildError  = data?.error;

  return (
    <TabShell>
      {/* ── State card ──────────────────────────────────────────────────── */}
      <div className="rounded-xl border border-border bg-card divide-y divide-border">
        {/* Active index row */}
        <InfoRow
          label="Index"
          value={
            activeType === "none"
              ? <span className="text-muted-foreground">None</span>
              : <span className="font-mono">{DISPLAY_NAMES[activeType] ?? activeType.toUpperCase()}</span>
          }
        />

        {/* Generation row — only when an active generation exists */}
        {activeGen !== undefined && (
          <InfoRow label="Generation" value={<span className="font-mono">{activeGen}</span>} />
        )}

        {/* Status row */}
        <InfoRow
          label="Status"
          value={<StatusBadge status={status} />}
        />

        {/* Search mode row */}
        <InfoRow
          label="Search"
          value={activeType === "none" ? "Exact (namespace scan)" : `ANN (${DISPLAY_NAMES[activeType] ?? activeType})`}
        />

        {/* Building replacement row */}
        {status === "building" && desiredType && (
          <InfoRow
            label="Building replacement"
            value={
              <span className="flex items-center gap-1.5">
                <span className="font-mono">{DISPLAY_NAMES[desiredType] ?? desiredType.toUpperCase()}</span>
                <span className="text-xs text-muted-foreground">(gen {buildingGen})</span>
              </span>
            }
          />
        )}

        {/* Informational note for building state */}
        {status === "building" && activeType !== "none" && (
          <div className="px-4 py-3 text-xs text-muted-foreground bg-accent/30">
            {DISPLAY_NAMES[activeType] ?? activeType.toUpperCase()} remains active while{" "}
            {desiredType ? DISPLAY_NAMES[desiredType] ?? desiredType.toUpperCase() : "the replacement"} builds.
            Search accuracy is unaffected.
          </div>
        )}

        {/* Failure detail */}
        {status === "failed" && buildError && (
          <div className="px-4 py-3 text-xs text-muted-foreground bg-red-500/5">
            <span className="font-medium text-red-700 dark:text-red-400">Build failed: </span>
            {buildError}
          </div>
        )}
      </div>

      {/* ── Action buttons (idle panel) ──────────────────────────────────── */}
      {panel === "idle" && (
        <div className="flex items-center gap-2 flex-wrap">
          {(status === "none" || status === "failed") && (
            <button
              onClick={() => { setPanel("create"); setActionError(null); }}
              className="px-3 py-1.5 text-xs font-medium rounded-lg bg-primary text-primary-foreground hover:bg-primary/80 transition-colors"
            >
              {status === "failed" && desiredType ? "Retry build" : "Create index"}
            </button>
          )}

          {status === "active" && (
            <>
              <button
                onClick={() => { setPanel("change"); setActionError(null); }}
                className="px-3 py-1.5 text-xs font-medium rounded-lg border border-border text-foreground hover:bg-accent transition-colors"
              >
                Change index
              </button>
              <button
                onClick={() => { setPanel("remove"); setActionError(null); }}
                className="px-3 py-1.5 text-xs font-medium rounded-lg border border-destructive/30 text-destructive hover:bg-destructive/10 transition-colors"
              >
                Remove index
              </button>
            </>
          )}

          {status === "building" && (
            <p className="text-xs text-muted-foreground">
              A build is in progress. This page refreshes automatically.
            </p>
          )}

          {status === "ready" && (
            <p className="text-xs text-muted-foreground">
              Build complete — activating automatically…
            </p>
          )}
        </div>
      )}

      {/* ── Create panel ────────────────────────────────────────────────── */}
      {panel === "create" && (
        <BuildForm
          title="Create index"
          submitLabel="Build index"
          onSubmit={handleBuild}
          onCancel={() => { setPanel("idle"); setActionError(null); }}
          pending={actionPending}
          error={actionError}
        />
      )}

      {/* ── Change panel ────────────────────────────────────────────────── */}
      {panel === "change" && (
        <BuildForm
          title="Change index"
          submitLabel="Build and switch"
          warning={
            activeType !== "none"
              ? `${DISPLAY_NAMES[activeType] ?? activeType.toUpperCase()} remains active while the new index builds. ` +
                `Search is unaffected during the build.`
              : undefined
          }
          onSubmit={handleBuild}
          onCancel={() => { setPanel("idle"); setActionError(null); }}
          pending={actionPending}
          error={actionError}
        />
      )}

      {/* ── Remove panel ────────────────────────────────────────────────── */}
      {panel === "remove" && (
        <RemovePanel
          activeType={activeType}
          onConfirm={handleRemove}
          onCancel={() => { setPanel("idle"); setActionError(null); }}
          pending={actionPending}
          error={actionError}
        />
      )}

    </TabShell>
  );
}
