"use client";

import { useState } from "react";
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Button } from "@/components/ui/button";
import {
  Plus, CircleCheck, Info, ChevronDown,
  Server, Network, Pencil,
} from "lucide-react";

const SHARD_OPTIONS = [1, 2, 4, 8];

import { useProjectManifest } from "@/lib/hooks/useProjectManifest";

interface Props {
  open: boolean;
  onClose: () => void;
  workspaceDir?: string | null;
  existingProjects?: string[];
  onCreate: (
    name: string,
    replication: 1 | 3,
    shardCount: number,
  ) => Promise<void>;
}

/* -- Shared step section header -------------------------------------- */

function StepHeader({
  n,
  title,
  subtitle,
  info,
}: {
  n: number;
  title: string;
  subtitle?: string;
  info?: string;
}) {
  return (
    <div className="flex items-start gap-2.5">
      <span className="mt-0.5 flex h-5 w-5 shrink-0 items-center justify-center rounded-full bg-[var(--v-accent)] text-[10px] font-semibold text-white">
        {n}
      </span>
      <div>
        <h3 className="text-sm font-semibold text-foreground">{title}</h3>
        {subtitle && (
          <p className="mt-0.5 flex items-center gap-1.5 text-xs text-muted-foreground">
            {subtitle}
            {info && (
              <span title={info} className="inline-flex shrink-0 cursor-help">
                <Info size={12} aria-label={info} />
              </span>
            )}
          </p>
        )}
      </div>
    </div>
  );
}

export function CreateProjectDialog({ open, onClose, workspaceDir, existingProjects, onCreate }: Props) {
  const { projects } = useProjectManifest();
  const knownProjectNames = existingProjects ?? (projects || []).map((p) => p.name);

  const [name, setName]               = useState("");
  const [replication, setReplication] = useState<1 | 3>(1);
  const [shardCount, setShardCount]   = useState(1);
  const [customShards, setCustomShards] = useState(false);
  const [busy, setBusy]               = useState(false);
  const [error, setError]             = useState("");

  const trimmedName = name.trim();
  const alreadyExists = Boolean(
    trimmedName &&
      knownProjectNames.some(
        (p) => p.toLowerCase() === trimmedName.toLowerCase()
      )
  );
  const nameFormatValid = trimmedName.length > 0 && /^[a-z0-9_-]+$/i.test(trimmedName);
  const nameValid = nameFormatValid && !alreadyExists;

  const submit = async () => {
    const n = name.trim();
    if (!n) { setError("Name is required"); return; }
    if (!/^[a-z0-9_-]+$/i.test(n)) {
      setError("Only letters, numbers, hyphens, underscores");
      return;
    }
    if (alreadyExists) {
      setError(`Project "${n}" already exists. Choose another name.`);
      return;
    }
    setBusy(true);
    setError("");
    try {
      // Sharding only applies to cluster mode — a single standalone node
      // has no shard concept at all, so pin it to 1 regardless.
      await onCreate(n, replication, replication === 3 ? shardCount : 1);
      setName("");
      onClose();
    } catch {
      setError("Failed to create project");
    } finally {
      setBusy(false);
    }
  };

  return (
    <Dialog open={open} onOpenChange={(o) => !o && onClose()}>
      <DialogContent className="bg-card border-input max-w-lg sm:max-w-xl p-0 max-h-[90vh] overflow-hidden flex flex-col">
        {/* Fixed header */}
        <div className="px-5 pt-5 pb-4 border-b border-border shrink-0">
          <DialogHeader className="flex-row items-center gap-3 pr-6">
            <div className="flex h-8 w-8 shrink-0 items-center justify-center rounded-lg bg-[var(--v-accent-muted)]">
              <Plus size={15} className="text-[var(--v-accent)]" />
            </div>
            <div>
              <DialogTitle className="text-foreground text-base font-semibold">New Project</DialogTitle>
              <p className="text-[11px] text-muted-foreground mt-0.5">
                Isolated node in{" "}
                <code className="font-mono text-[10px]">
                  {workspaceDir ? `${workspaceDir}/projects` : "your workspace"}
                </code>
              </p>
            </div>
          </DialogHeader>
        </div>

        {/* Scrollable body */}
        <div className="overflow-y-auto flex-1 px-5 py-4">
          <div className="flex flex-col gap-4">
            {/* 1. Project name */}
            <div className="flex flex-col gap-2">
              <StepHeader n={1} title="Project name" subtitle="Choose a unique name for your project." />
              <div className="relative">
                <input
                  autoFocus
                  autoCapitalize="none"
                  autoCorrect="off"
                  spellCheck={false}
                  value={name}
                  onChange={(e) => {
                    setName(e.target.value);
                    setError("");
                  }}
                  onKeyDown={(e) => e.key === "Enter" && nameValid && !busy && submit()}
                  placeholder="my-project"
                  className={`w-full rounded-lg border bg-background px-3 py-2 pr-9 text-sm text-foreground placeholder:text-muted-foreground focus:outline-none focus:ring-1 ${
                    trimmedName && !nameFormatValid
                      ? "border-red-500 focus:ring-red-500/30"
                      : alreadyExists
                      ? "border-amber-500 focus:ring-amber-500/30"
                      : "border-input focus:ring-ring"
                  }`}
                />
                {nameValid && (
                  <CircleCheck size={15} className="absolute right-3 top-1/2 -translate-y-1/2 text-emerald-500" />
                )}
              </div>
              {trimmedName && !nameFormatValid && (
                <p className="text-xs text-red-500">
                  Invalid character — use only letters, numbers, hyphens, and underscores.
                </p>
              )}
              {alreadyExists && (
                <p className="text-xs text-amber-500 font-medium">
                  A project named &quot;{trimmedName}&quot; already exists. Please choose a different name.
                </p>
              )}
            </div>

            <div className="border-t border-border" />

            {/* 2. Replication */}
            <div className="flex flex-col gap-2">
              <StepHeader n={2} title="Replication" subtitle="Choose how your data is replicated for availability." />
              <div className="flex gap-2">
                <button
                  type="button"
                  onClick={() => setReplication(1)}
                  className={`flex flex-1 items-center gap-2.5 rounded-lg border px-3 py-2.5 text-left transition-colors ${
                    replication === 1
                      ? "border-[var(--v-accent)] bg-[var(--v-accent-muted)]"
                      : "border-input bg-background hover:border-muted-foreground/40"
                  }`}
                >
                  <span className={`flex h-3.5 w-3.5 shrink-0 items-center justify-center rounded-full border ${
                    replication === 1 ? "border-[var(--v-accent)]" : "border-muted-foreground/40"
                  }`}>
                    {replication === 1 && <span className="h-1.5 w-1.5 rounded-full bg-[var(--v-accent)]" />}
                  </span>
                  <Server size={14} className="shrink-0 text-muted-foreground" />
                  <div>
                    <p className="text-xs font-medium text-foreground">Single Node</p>
                    <p className="text-[10px] text-muted-foreground">One process, no replication</p>
                  </div>
                </button>
                <button
                  type="button"
                  onClick={() => setReplication(3)}
                  className={`flex flex-1 items-center gap-2.5 rounded-lg border px-3 py-2.5 text-left transition-colors ${
                    replication === 3
                      ? "border-[var(--v-accent)] bg-[var(--v-accent-muted)]"
                      : "border-input bg-background hover:border-muted-foreground/40"
                  }`}
                >
                  <span className={`flex h-3.5 w-3.5 shrink-0 items-center justify-center rounded-full border ${
                    replication === 3 ? "border-[var(--v-accent)]" : "border-muted-foreground/40"
                  }`}>
                    {replication === 3 && <span className="h-1.5 w-1.5 rounded-full bg-[var(--v-accent)]" />}
                  </span>
                  <Network size={14} className="shrink-0 text-muted-foreground" />
                  <div>
                    <p className="text-xs font-medium text-foreground">3-Node Cluster</p>
                    <p className="text-[10px] text-muted-foreground">Raft-replicated, tolerates 1 node down</p>
                  </div>
                </button>
              </div>
            </div>

            {/* 3. Shards — only for cluster */}
            {replication === 3 && (
              <>
                <div className="border-t border-border" />
                <div className="flex flex-col gap-2">
                  <StepHeader
                    n={3}
                    title="Shards"
                    subtitle="Splits collections across N independent partitions within each replica."
                    info="Each shard runs its own Raft group; namespaces route to a shard by name hash."
                  />
                  <div className="grid grid-cols-5 gap-1.5">
                    {SHARD_OPTIONS.map((n) => (
                      <button
                        key={n}
                        type="button"
                        onClick={() => { setShardCount(n); setCustomShards(false); }}
                        className={`rounded-lg border px-3 py-2 text-xs font-medium transition-colors ${
                          !customShards && shardCount === n
                            ? "border-[var(--v-accent)] bg-[var(--v-accent-muted)] text-foreground"
                            : "border-input bg-background text-muted-foreground hover:text-foreground"
                        }`}
                      >
                        {n}
                      </button>
                    ))}
                    {customShards ? (
                      <div className="flex items-center justify-center gap-1 rounded-lg border border-[var(--v-accent)] bg-[var(--v-accent-muted)] px-2 py-1.5">
                        <input
                          type="number"
                          min={1}
                          max={64}
                          autoFocus
                          value={shardCount}
                          onChange={(e) => setShardCount(Math.max(1, Number(e.target.value) || 1))}
                          className="w-full bg-transparent text-center text-xs font-medium text-foreground focus:outline-none"
                        />
                      </div>
                    ) : (
                      <button
                        type="button"
                        onClick={() => setCustomShards(true)}
                        className="flex items-center justify-center gap-1.5 rounded-lg border border-input bg-background px-2 py-2 text-xs font-medium text-muted-foreground hover:text-foreground transition-colors"
                      >
                        Custom <Pencil size={11} />
                      </button>
                    )}
                  </div>
                  <div className="flex items-start gap-2 rounded-lg border border-[var(--v-accent)]/20 bg-[var(--v-accent-muted)] px-3 py-2">
                    <Info size={12} className="mt-0.5 shrink-0 text-[var(--v-accent)]" />
                    <p className="text-[11px] leading-relaxed text-foreground/80">
                      More shards increase write throughput and capacity. Proof and Timeline currently only
                      reflect the default shard; per-shard views are a planned follow-up.
                    </p>
                  </div>
                </div>
              </>
            )}

            {/* Collections hint */}
            <div className="border-t border-border" />
            <div className="flex items-start gap-2 rounded-lg border border-border bg-muted/30 px-3 py-2">
              <Info size={12} className="mt-0.5 shrink-0 text-muted-foreground" />
              <p className="text-[11px] leading-relaxed text-muted-foreground">
                Vector dimensions are configured per-collection after creating the project.
              </p>
            </div>
          </div>
        </div>

        {/* Fixed footer */}
        <div className="border-t border-border px-5 py-3 flex items-center justify-between shrink-0">
          {error
            ? <p className="text-xs text-red-600">{error}</p>
            : <span />
          }
          <div className="flex gap-2">
            <Button variant="ghost" size="sm" onClick={onClose} className="text-muted-foreground hover:text-foreground">
              Cancel
            </Button>
            <Button size="sm" onClick={submit} disabled={busy || !nameValid}
              className="bg-primary text-primary-foreground hover:bg-primary/90">
              {busy ? "Creating…" : "Create & open"}
            </Button>
          </div>
        </div>
      </DialogContent>
    </Dialog>
  );
}
