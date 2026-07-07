"use client";

import { useState } from "react";
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Button } from "@/components/ui/button";
import { DIMENSIONS, DEFAULT_DIMENSION } from "@/lib/dimensions";

const SHARD_OPTIONS = [1, 2, 4, 8];

export interface EmbedPreset {
  provider: string;
  model: string;
  endpoint?: string;
}

interface Props {
  open: boolean;
  onClose: () => void;
  onCreate: (
    name: string,
    dim: number,
    index: "brute" | "hnsw" | "ivf" | "bq" | "auto",
    replication: 1 | 3,
    shardCount: number,
    embed?: EmbedPreset
  ) => Promise<void>;
}

const MODEL_PRESETS: { label: string; dim: number; provider: string; model: string; endpoint?: string }[] = [
  { label: "nomic-embed-text",   dim: 768,  provider: "ollama", model: "nomic-embed-text",       endpoint: "http://localhost:11434/api/embed" },
  { label: "text-embed-3-small", dim: 1536, provider: "openai", model: "text-embedding-3-small", endpoint: "https://api.openai.com/v1/embeddings" },
  { label: "text-embed-ada-002", dim: 1536, provider: "openai", model: "text-embedding-ada-002", endpoint: "https://api.openai.com/v1/embeddings" },
  { label: "mxbai-embed-large",  dim: 1024, provider: "ollama", model: "mxbai-embed-large",      endpoint: "http://localhost:11434/api/embed" },
  { label: "bge-small-en",       dim: 384,  provider: "ollama", model: "bge-small-en",           endpoint: "http://localhost:11434/api/embed" },
  { label: "all-MiniLM-L6-v2",  dim: 384,  provider: "ollama", model: "all-minilm",             endpoint: "http://localhost:11434/api/embed" },
];

export function CreateProjectDialog({ open, onClose, onCreate }: Props) {
  const [name, setName]               = useState("");
  const [dim, setDim]                 = useState(DEFAULT_DIMENSION);
  const [index, setIndex]             = useState<"brute" | "hnsw" | "ivf" | "bq" | "auto">("brute");
  const [replication, setReplication] = useState<1 | 3>(1);
  const [shardCount, setShardCount]   = useState(1);
  const [busy, setBusy]               = useState(false);
  const [error, setError]             = useState("");
  const [selectedEmbed, setSelectedEmbed] = useState<EmbedPreset | undefined>(
    MODEL_PRESETS.find(p => p.dim === DEFAULT_DIMENSION)
      ? { provider: MODEL_PRESETS.find(p => p.dim === DEFAULT_DIMENSION)!.provider,
          model:    MODEL_PRESETS.find(p => p.dim === DEFAULT_DIMENSION)!.model,
          endpoint: MODEL_PRESETS.find(p => p.dim === DEFAULT_DIMENSION)!.endpoint }
      : undefined
  );

  const submit = async () => {
    const n = name.trim();
    if (!n) { setError("Name is required"); return; }
    if (!/^[a-z0-9_-]+$/i.test(n)) {
      setError("Only letters, numbers, hyphens, underscores");
      return;
    }
    setBusy(true);
    setError("");
    try {
      // Sharding only applies to cluster mode — a single standalone node
      // has no shard concept at all, so pin it to 1 regardless of the
      // control's last value (matches the server-side pin in createProject).
      await onCreate(n, dim, index, replication, replication === 3 ? shardCount : 1, selectedEmbed);
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
      <DialogContent className="bg-card border-input max-w-lg">
        <DialogHeader>
          <DialogTitle className="text-foreground text-base">New Project</DialogTitle>
        </DialogHeader>
        <div className="flex flex-col gap-3 pt-1">
          <p className="text-xs text-muted-foreground">
            An isolated, persistent vector store with its own node, port, and data dir
            under <code className="font-mono">~/.valori/projects</code>.
          </p>

          <div className="flex flex-col gap-1">
            <label className="text-[10px] text-muted-foreground uppercase tracking-wider">Name</label>
            <input
              autoFocus
              value={name}
              onChange={(e) => setName(e.target.value)}
              onKeyDown={(e) => e.key === "Enter" && submit()}
              placeholder="my-project"
              className="rounded-md border border-input bg-background px-3 py-2 text-sm text-foreground placeholder:text-muted-foreground focus:outline-none focus:ring-1 focus:ring-ring"
            />
          </div>

          <div className="flex flex-col gap-1.5">
            <label className="text-[10px] text-muted-foreground uppercase tracking-wider">
              Dimension <span className="normal-case opacity-70">· permanent, must match your embedding model</span>
            </label>
            {/* Model preset quick-pick */}
            <div className="flex flex-wrap gap-1.5">
              {MODEL_PRESETS.map((p) => (
                <button
                  key={p.label}
                  type="button"
                  onClick={() => {
                    setDim(p.dim);
                    setSelectedEmbed({ provider: p.provider, model: p.model, endpoint: p.endpoint });
                  }}
                  className={`px-2 py-0.5 rounded border text-[10px] font-mono transition-colors ${
                    dim === p.dim && selectedEmbed?.model === p.model
                      ? "border-[var(--v-accent)] bg-[var(--v-accent-muted)] text-foreground"
                      : "border-input text-muted-foreground hover:text-foreground hover:border-muted-foreground/40"
                  }`}
                >
                  {p.label} <span className="opacity-60">({p.dim})</span>
                </button>
              ))}
            </div>
            <select
              value={dim}
              onChange={(e) => {
                const d = Number(e.target.value);
                setDim(d);
                const match = MODEL_PRESETS.find(p => p.dim === d);
                setSelectedEmbed(match ? { provider: match.provider, model: match.model, endpoint: match.endpoint } : undefined);
              }}
              className="rounded-md border border-input bg-background px-3 py-2 text-sm text-foreground font-mono focus:outline-none focus:ring-1 focus:ring-ring appearance-none cursor-pointer"
            >
              {DIMENSIONS.map((d) => (
                <option key={d.value} value={d.value}>{d.label}</option>
              ))}
            </select>
            <p className="text-[10px] text-amber-600 dark:text-amber-500">
              ⚠ Cannot be changed after the first vector is inserted
            </p>
          </div>

          <div className="flex flex-col gap-1">
            <label className="text-[10px] text-muted-foreground uppercase tracking-wider">Index</label>
            <div className="flex gap-2">
              {(["auto", "brute", "hnsw", "ivf", "bq"] as const).map((opt) => (
                <button
                  key={opt}
                  type="button"
                  onClick={() => setIndex(opt)}
                  title={opt === "auto" ? "Auto: brute-force < 10k · BQ 10k–2M · HNSW > 2M" : undefined}
                  className={`flex-1 rounded-md border px-3 py-1.5 text-xs transition-colors ${
                    index === opt
                      ? "border-[var(--v-accent)] bg-[var(--v-accent-muted)] text-foreground"
                      : "border-input bg-background text-muted-foreground hover:text-foreground"
                  }`}
                >
                  {opt === "auto" ? "Auto" : opt === "brute" ? "Brute" : opt === "hnsw" ? "HNSW" : opt === "ivf" ? "IVF" : "BQ"}
                </button>
              ))}
            </div>
          </div>

          <div className="flex flex-col gap-1">
            <label className="text-[10px] text-muted-foreground uppercase tracking-wider">Replication</label>
            <div className="flex gap-2">
              <button
                type="button"
                onClick={() => setReplication(1)}
                className={`flex-1 rounded-md border px-3 py-2 text-left transition-colors ${
                  replication === 1
                    ? "border-[var(--v-accent)] bg-[var(--v-accent-muted)]"
                    : "border-input bg-background hover:border-muted-foreground/40"
                }`}
              >
                <p className="text-xs font-medium text-foreground">Single Node</p>
                <p className="text-[10px] text-muted-foreground mt-0.5">One process, no replication</p>
              </button>
              <button
                type="button"
                onClick={() => setReplication(3)}
                className={`flex-1 rounded-md border px-3 py-2 text-left transition-colors ${
                  replication === 3
                    ? "border-[var(--v-accent)] bg-[var(--v-accent-muted)]"
                    : "border-input bg-background hover:border-muted-foreground/40"
                }`}
              >
                <p className="text-xs font-medium text-foreground">3-Node Cluster</p>
                <p className="text-[10px] text-muted-foreground mt-0.5">Raft-replicated, tolerates 1 node down</p>
              </button>
            </div>
          </div>

          {replication === 3 && (
            <div className="flex flex-col gap-1">
              <label className="text-[10px] text-muted-foreground uppercase tracking-wider">Shards</label>
              <div className="flex gap-2">
                {SHARD_OPTIONS.map((n) => (
                  <button
                    key={n}
                    type="button"
                    onClick={() => setShardCount(n)}
                    className={`flex-1 rounded-md border px-3 py-1.5 text-xs transition-colors ${
                      shardCount === n
                        ? "border-[var(--v-accent)] bg-[var(--v-accent-muted)] text-foreground"
                        : "border-input bg-background text-muted-foreground hover:text-foreground"
                    }`}
                  >
                    {n}
                  </button>
                ))}
              </div>
              <p className="text-[10px] text-muted-foreground/70 leading-relaxed">
                Splits collections across {shardCount > 1 ? shardCount : "N"} independent partitions within
                each replica — same fault tolerance, more capacity. Proof and Timeline currently only reflect
                the default shard; per-shard views are a planned follow-up.
              </p>
            </div>
          )}

          {error && <p className="text-xs text-red-700">{error}</p>}
          <div className="flex gap-2 justify-end">
            <Button
              variant="ghost"
              size="sm"
              onClick={onClose}
              className="text-muted-foreground hover:text-foreground"
            >
              Cancel
            </Button>
            <Button
              size="sm"
              onClick={submit}
              disabled={busy}
              className="bg-primary text-primary-foreground hover:bg-primary/90"
            >
              {busy ? "Creating…" : "Create & open"}
            </Button>
          </div>
        </div>
      </DialogContent>
    </Dialog>
  );
}
