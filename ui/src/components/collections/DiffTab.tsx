"use client";

import { useState, useCallback, useEffect } from "react";
import { TabShell } from "@/components/collections/TabShell";

// --- Types --------------------------------------------------------------------

interface NsSnapshot {
  namespace: string;
  record_ids: number[];
  node_ids: number[];
  event_count: number;
  ns_event_count: number;
  record_count: number;
  node_count: number;
  ns_proof_hash: string;
  global_hash: string | null;
  error?: string;
}

interface DiffResult {
  a: NsSnapshot;
  b: NsSnapshot;
  onlyInA: number[];    // removed if A is "before"
  onlyInB: number[];    // added if B is "after"
  inBoth: number[];     // unchanged
  nodeOnlyInA: number[];
  nodeOnlyInB: number[];
  nodeInBoth: number[];
  hashMatch: boolean;
  computed_at: string;
}

// --- Fetch helpers ------------------------------------------------------------

async function fetchSnapshot(namespace: string): Promise<NsSnapshot> {
  const res = await fetch(
    `/api/namespace-audit?namespace=${encodeURIComponent(namespace)}`,
    { cache: "no-store" }
  );
  if (!res.ok) throw new Error(`Audit failed for "${namespace}" (${res.status})`);
  const d = await res.json() as {
    ns_record_ids: number[];
    ns_node_ids: number[];
    record_count: number;
    node_count: number;
    total_events: number;
    ns_event_ids: number[];
    ns_proof_hash: string;
    global_state_hash: string | null;
    error?: string;
  };
  if (d.error) throw new Error(d.error);
  return {
    namespace,
    record_ids: d.ns_record_ids,
    node_ids: d.ns_node_ids,
    event_count: d.total_events,
    ns_event_count: d.ns_event_ids.length,
    record_count: d.record_count,
    node_count: d.node_count,
    ns_proof_hash: d.ns_proof_hash,
    global_hash: d.global_state_hash,
  };
}

function computeDiff(a: NsSnapshot, b: NsSnapshot): DiffResult {
  const setA = new Set(a.record_ids);
  const setB = new Set(b.record_ids);
  const setNA = new Set(a.node_ids);
  const setNB = new Set(b.node_ids);

  const onlyInA = a.record_ids.filter((id) => !setB.has(id));
  const onlyInB = b.record_ids.filter((id) => !setA.has(id));
  const inBoth = a.record_ids.filter((id) => setB.has(id));

  const nodeOnlyInA = a.node_ids.filter((id) => !setNB.has(id));
  const nodeOnlyInB = b.node_ids.filter((id) => !setNA.has(id));
  const nodeInBoth = a.node_ids.filter((id) => setNB.has(id));

  return {
    a,
    b,
    onlyInA,
    onlyInB,
    inBoth,
    nodeOnlyInA,
    nodeOnlyInB,
    nodeInBoth,
    hashMatch: a.ns_proof_hash === b.ns_proof_hash,
    computed_at: new Date().toISOString(),
  };
}

// --- ID chips -----------------------------------------------------------------

function IdChips({
  ids,
  color,
  limit = 120,
}: {
  ids: number[];
  color: string;
  limit?: number;
}) {
  const [expanded, setExpanded] = useState(false);
  const shown = expanded ? ids : ids.slice(0, limit);
  const hidden = ids.length - shown.length;

  if (ids.length === 0) {
    return <span className="text-xs text-muted-foreground italic">none</span>;
  }

  return (
    <div className="flex flex-wrap gap-1">
      {shown.map((id) => (
        <span
          key={id}
          className={`font-mono text-[10px] px-1.5 py-0.5 rounded border ${color}`}
        >
          #{id}
        </span>
      ))}
      {hidden > 0 && (
        <button
          onClick={() => setExpanded(true)}
          className="font-mono text-[10px] px-1.5 py-0.5 rounded border border-input text-muted-foreground hover:text-accent-foreground transition-colors"
        >
          +{hidden} more
        </button>
      )}
    </div>
  );
}

// --- Namespace selector -------------------------------------------------------

function NsSelector({
  label,
  value,
  onChange,
  namespaces,
}: {
  label: string;
  value: string;
  onChange: (v: string) => void;
  namespaces: string[];
}) {
  return (
    <div className="flex flex-col gap-1.5 flex-1 min-w-0">
      <p className="text-[10px] text-muted-foreground uppercase tracking-widest">{label}</p>
      <div className="relative">
        <select
          value={value}
          onChange={(e) => onChange(e.target.value)}
          className="w-full bg-accent border border-input text-accent-foreground text-sm rounded-lg px-3 py-2 appearance-none focus:outline-none focus:border-ring"
        >
          <option value="">— select namespace —</option>
          {namespaces.map((ns) => (
            <option key={ns} value={ns}>
              {ns}
            </option>
          ))}
        </select>
        <span className="absolute right-2.5 top-1/2 -translate-y-1/2 text-muted-foreground pointer-events-none text-xs">▾</span>
      </div>
      {/* Manual input fallback */}
      <input
        type="text"
        value={value}
        onChange={(e) => onChange(e.target.value)}
        placeholder="or type namespace manually…"
        className="text-xs bg-accent/50 border border-input/50 text-muted-foreground rounded px-2.5 py-1.5 focus:outline-none focus:border-muted placeholder:text-muted-foreground"
      />
    </div>
  );
}

// --- Snapshot card ------------------------------------------------------------

function SnapshotCard({
  snap,
  label,
  accentColor,
}: {
  snap: NsSnapshot;
  label: string;
  accentColor: string;
}) {
  return (
    <div className={`rounded-xl border bg-background p-4 flex flex-col gap-3 flex-1 ${accentColor}`}>
      <div className="flex items-center gap-2">
        <span className={`text-xs font-mono px-2 py-0.5 rounded border ${accentColor}`}>{label}</span>
        <p className="text-xs text-muted-foreground font-mono truncate">{snap.namespace}</p>
      </div>
      <div className="grid grid-cols-2 gap-x-4 gap-y-1.5 text-xs">
        {[
          ["Records", snap.record_count.toLocaleString()],
          ["Nodes", snap.node_count.toLocaleString()],
          ["NS events", snap.ns_event_count.toLocaleString()],
          ["Total events", snap.event_count.toLocaleString()],
        ].map(([k, v]) => (
          <div key={k}>
            <span className="text-muted-foreground">{k} </span>
            <span className="text-accent-foreground font-semibold">{v}</span>
          </div>
        ))}
      </div>
      <div>
        <p className="text-[9px] text-muted-foreground uppercase tracking-widest mb-0.5">NS Proof Hash</p>
        <p className="font-mono text-[9.5px] text-muted-foreground break-all">
          {snap.ns_proof_hash.slice(0, 40)}…
        </p>
      </div>
    </div>
  );
}

// --- Diff section -------------------------------------------------------------

function DiffSection({
  title,
  count,
  nodeCount,
  ids,
  nodeIds,
  chipColor,
  icon,
  defaultOpen = false,
}: {
  title: string;
  count: number;
  nodeCount: number;
  ids: number[];
  nodeIds: number[];
  chipColor: string;
  icon: string;
  defaultOpen?: boolean;
}) {
  const [open, setOpen] = useState(defaultOpen);

  return (
    <div className="rounded-xl border border-border bg-card overflow-hidden">
      <button
        onClick={() => setOpen((v) => !v)}
        className="w-full flex items-center justify-between px-4 py-3 hover:bg-accent/50 transition-colors"
      >
        <div className="flex items-center gap-3">
          <span className="text-base">{icon}</span>
          <span className="text-sm font-medium text-card-foreground">{title}</span>
          <span className={`text-xs font-mono px-2 py-0.5 rounded-full ${chipColor}`}>
            {count} records{nodeCount > 0 ? ` · ${nodeCount} nodes` : ""}
          </span>
        </div>
        <span className="text-muted-foreground text-xs">{open ? "▲" : "▼"}</span>
      </button>

      {open && (
        <div className="px-4 pb-4 flex flex-col gap-3">
          {ids.length > 0 && (
            <div>
              <p className="text-[9px] text-muted-foreground uppercase tracking-widest mb-2">Record IDs</p>
              <IdChips ids={ids} color={chipColor} />
            </div>
          )}
          {nodeIds.length > 0 && (
            <div>
              <p className="text-[9px] text-muted-foreground uppercase tracking-widest mb-2">Graph Node IDs</p>
              <IdChips ids={nodeIds} color="border-input text-muted-foreground" />
            </div>
          )}
          {ids.length === 0 && nodeIds.length === 0 && (
            <p className="text-xs text-muted-foreground italic">none</p>
          )}
        </div>
      )}
    </div>
  );
}

// --- Download helpers ---------------------------------------------------------

function downloadDiff(diff: DiffResult) {
  const blob = new Blob([JSON.stringify(diff, null, 2)], { type: "application/json" });
  const url = URL.createObjectURL(blob);
  const a = document.createElement("a");
  a.href = url;
  a.download = `valori-diff-${Date.now()}.json`;
  a.click();
  URL.revokeObjectURL(url);
}

// --- Main tab -----------------------------------------------------------------

export function DiffTab({ namespace }: { namespace: string }) {
  const [namespaces, setNamespaces] = useState<string[]>([]);
  const [nsA, setNsA] = useState(namespace);
  const [nsB, setNsB] = useState("");
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [diff, setDiff] = useState<DiffResult | null>(null);
  const [diffView, setDiffView] = useState<"records" | "graph">("records");

  // Load namespace list
  useEffect(() => {
    fetch("/api/namespaces", { cache: "no-store" })
      .then((r) => r.ok ? r.json() : [])
      .then((d) => {
        const list: string[] = Array.isArray(d)
          ? d
          : Array.isArray(d?.namespaces)
          ? d.namespaces
          : [];
        setNamespaces(list);
      })
      .catch(() => {});
  }, []);

  const compare = useCallback(async () => {
    if (!nsA || !nsB) return;
    setLoading(true);
    setError(null);
    setDiff(null);
    try {
      const [snapA, snapB] = await Promise.all([
        fetchSnapshot(nsA),
        fetchSnapshot(nsB),
      ]);
      setDiff(computeDiff(snapA, snapB));
    } catch (e) {
      setError(e instanceof Error ? e.message : "Diff failed");
    } finally {
      setLoading(false);
    }
  }, [nsA, nsB]);

  const swap = useCallback(() => {
    setNsA(nsB);
    setNsB(nsA);
    setDiff(null);
  }, [nsA, nsB]);

  return (
    <TabShell>

      {/* Selectors */}
      <div className="rounded-xl border border-border bg-card p-5">
        <p className="text-sm font-semibold text-card-foreground mb-4">Compare Namespaces</p>
        <div className="flex items-end gap-3">
          <NsSelector
            label="Namespace A (base)"
            value={nsA}
            onChange={(v) => { setNsA(v); setDiff(null); }}
            namespaces={namespaces}
          />
          <div className="flex flex-col items-center gap-1 pb-2 flex-shrink-0">
            <button
              onClick={swap}
              title="Swap A and B"
              className="text-muted-foreground hover:text-accent-foreground transition-colors text-lg"
            >
              ⇄
            </button>
          </div>
          <NsSelector
            label="Namespace B (compare)"
            value={nsB}
            onChange={(v) => { setNsB(v); setDiff(null); }}
            namespaces={namespaces}
          />
        </div>
        <div className="flex items-center gap-3 mt-4">
          <button
            onClick={compare}
            disabled={loading || !nsA || !nsB}
            className="px-5 py-2 rounded-lg bg-primary text-primary-foreground text-sm font-medium hover:bg-primary/90 disabled:opacity-40 disabled:cursor-not-allowed transition-colors"
          >
            {loading ? "Comparing…" : "Compare →"}
          </button>
          {nsA === nsB && nsA && (
            <p className="text-xs text-muted-foreground">Select two different namespaces</p>
          )}
          {error && <p className="text-xs text-red-400">{error}</p>}
        </div>
      </div>

      {/* Loading */}
      {loading && (
        <div className="flex flex-col gap-2">
          {Array.from({ length: 3 }).map((_, i) => (
            <div key={i} className="h-12 bg-accent rounded-xl animate-pulse" />
          ))}
        </div>
      )}

      {/* Results */}
      {diff && (
        <>
          {/* Snapshot cards */}
          <div className="flex gap-3">
            <SnapshotCard
              snap={diff.a}
              label="A"
              accentColor="border-blue-900/60 text-blue-400"
            />
            <SnapshotCard
              snap={diff.b}
              label="B"
              accentColor="border-purple-900/60 text-purple-400"
            />
          </div>

          {/* Summary bar */}
          <div className="rounded-xl border border-border bg-card px-5 py-4 flex flex-col gap-3">
            <div className="flex items-center justify-between">
              <div className="flex items-center gap-6">
                <div className="flex items-center gap-2">
                  <span className="text-red-400 text-lg">−</span>
                  <div>
                    <p className="text-xl font-bold text-red-400">{diff.onlyInA.length}</p>
                    <p className="text-[10px] text-muted-foreground uppercase tracking-widest">only in A</p>
                  </div>
                </div>
                <div className="h-8 border-l border-input" />
                <div className="flex items-center gap-2">
                  <span className="text-emerald-400 text-lg">+</span>
                  <div>
                    <p className="text-xl font-bold text-emerald-400">{diff.onlyInB.length}</p>
                    <p className="text-[10px] text-muted-foreground uppercase tracking-widest">only in B</p>
                  </div>
                </div>
                <div className="h-8 border-l border-input" />
                <div className="flex items-center gap-2">
                  <span className="text-muted-foreground text-lg">=</span>
                  <div>
                    <p className="text-xl font-bold text-muted-foreground">{diff.inBoth.length}</p>
                    <p className="text-[10px] text-muted-foreground uppercase tracking-widest">in both</p>
                  </div>
                </div>
              </div>

              <div className="flex items-center gap-3">
                {/* Hash match pill */}
                <span
                  className={`text-xs px-2.5 py-1 rounded-full border font-mono ${
                    diff.hashMatch
                      ? "border-emerald-800 bg-emerald-950/40 text-emerald-400"
                      : "border-red-800 bg-red-950/40 text-red-400"
                  }`}
                >
                  {diff.hashMatch ? "✓ hashes match" : "✗ hashes differ"}
                </span>
                <button
                  onClick={() => downloadDiff(diff)}
                  className="text-xs px-3 py-1.5 rounded border border-input text-muted-foreground hover:text-foreground hover:border-ring transition-all"
                >
                  download JSON
                </button>
              </div>
            </div>

            {/* Graph summary if relevant */}
            {(diff.nodeOnlyInA.length > 0 || diff.nodeOnlyInB.length > 0) && (
              <div className="border-t border-border pt-3 flex gap-6 text-xs">
                <span className="text-muted-foreground">Graph nodes:</span>
                <span className="text-red-400">−{diff.nodeOnlyInA.length} only in A</span>
                <span className="text-emerald-400">+{diff.nodeOnlyInB.length} only in B</span>
                <span className="text-muted-foreground">={diff.nodeInBoth.length} common</span>
              </div>
            )}

            {/* View toggle */}
            <div className="flex items-center gap-0.5 bg-accent rounded-md border border-input p-0.5 w-fit">
              {(["records", "graph"] as const).map((v) => (
                <button
                  key={v}
                  onClick={() => setDiffView(v)}
                  className={`px-3 py-0.5 text-xs rounded transition-colors ${
                    diffView === v
                      ? "bg-muted text-foreground"
                      : "text-muted-foreground hover:text-accent-foreground"
                  }`}
                >
                  {v}
                </button>
              ))}
            </div>
          </div>

          {/* Diff sections */}
          {diffView === "records" ? (
            <div className="flex flex-col gap-3">
              <DiffSection
                title="Only in A"
                count={diff.onlyInA.length}
                nodeCount={0}
                ids={diff.onlyInA}
                nodeIds={[]}
                chipColor="border-red-900/60 bg-red-950/20 text-red-400"
                icon="−"
                defaultOpen={diff.onlyInA.length > 0 && diff.onlyInA.length <= 200}
              />
              <DiffSection
                title="Only in B"
                count={diff.onlyInB.length}
                nodeCount={0}
                ids={diff.onlyInB}
                nodeIds={[]}
                chipColor="border-emerald-900/60 bg-emerald-950/20 text-emerald-400"
                icon="+"
                defaultOpen={diff.onlyInB.length > 0 && diff.onlyInB.length <= 200}
              />
              <DiffSection
                title="In both namespaces"
                count={diff.inBoth.length}
                nodeCount={0}
                ids={diff.inBoth}
                nodeIds={[]}
                chipColor="border-input bg-accent text-muted-foreground"
                icon="="
                defaultOpen={false}
              />
            </div>
          ) : (
            <div className="flex flex-col gap-3">
              <DiffSection
                title="Graph nodes only in A"
                count={diff.nodeOnlyInA.length}
                nodeCount={diff.nodeOnlyInA.length}
                ids={[]}
                nodeIds={diff.nodeOnlyInA}
                chipColor="border-red-900/60 bg-red-950/20 text-red-400"
                icon="−"
                defaultOpen={diff.nodeOnlyInA.length > 0 && diff.nodeOnlyInA.length <= 200}
              />
              <DiffSection
                title="Graph nodes only in B"
                count={diff.nodeOnlyInB.length}
                nodeCount={diff.nodeOnlyInB.length}
                ids={[]}
                nodeIds={diff.nodeOnlyInB}
                chipColor="border-emerald-900/60 bg-emerald-950/20 text-emerald-400"
                icon="+"
                defaultOpen={diff.nodeOnlyInB.length > 0 && diff.nodeOnlyInB.length <= 200}
              />
              <DiffSection
                title="Graph nodes in both"
                count={diff.nodeInBoth.length}
                nodeCount={diff.nodeInBoth.length}
                ids={[]}
                nodeIds={diff.nodeInBoth}
                chipColor="border-input bg-accent text-muted-foreground"
                icon="="
                defaultOpen={false}
              />
            </div>
          )}

          {/* Proof hash diff */}
          <div className="rounded-xl border border-border bg-card p-4 flex flex-col gap-3">
            <p className="text-xs font-medium text-muted-foreground">Namespace Proof Hashes</p>
            <div className="grid grid-cols-1 gap-2 sm:grid-cols-2">
              {[
                { label: "A — " + diff.a.namespace, hash: diff.a.ns_proof_hash, color: "text-blue-400" },
                { label: "B — " + diff.b.namespace, hash: diff.b.ns_proof_hash, color: "text-purple-400" },
              ].map(({ label, hash, color }) => (
                <div key={label} className="rounded-lg bg-background border border-border px-3 py-2">
                  <p className="text-[9px] text-muted-foreground uppercase tracking-widest mb-1 truncate">{label}</p>
                  <p className={`font-mono text-[10px] break-all ${color}`}>{hash}</p>
                </div>
              ))}
            </div>
            <p className="text-[10px] text-muted-foreground">
              {diff.hashMatch
                ? "Identical proof hashes confirm these namespaces share the same event history."
                : "Proof hashes differ — the namespaces have different event histories or record sets."}
            </p>
          </div>

          <p className="text-[10px] text-muted-foreground text-right font-mono">
            computed at {new Date(diff.computed_at).toLocaleString()}
          </p>
        </>
      )}
    </TabShell>
  );
}
