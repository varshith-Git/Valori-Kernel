"use client";

import { useState } from "react";
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Button } from "@/components/ui/button";

interface Props {
  open: boolean;
  onClose: () => void;
  onCreate: (name: string, dim: number, index: "brute" | "hnsw" | "ivf") => Promise<void>;
}

const DIMS = [
  { value: 384,  label: "384 — MiniLM / paraphrase" },
  { value: 768,  label: "768 — BERT-base / mpnet / nomic" },
  { value: 1024, label: "1024 — bge-large / BERT-large" },
  { value: 1536, label: "1536 — text-embedding-ada-002" },
  { value: 3072, label: "3072 — text-embedding-3-large" },
];

export function CreateProjectDialog({ open, onClose, onCreate }: Props) {
  const [name, setName]   = useState("");
  const [dim, setDim]     = useState(768);
  const [index, setIndex] = useState<"brute" | "hnsw" | "ivf">("brute");
  const [busy, setBusy]   = useState(false);
  const [error, setError] = useState("");

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
      await onCreate(n, dim, index);
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
      <DialogContent className="bg-card border-input max-w-sm">
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

          <div className="flex flex-col gap-1">
            <label className="text-[10px] text-muted-foreground uppercase tracking-wider">
              Dimension <span className="normal-case opacity-70">· immutable after first insert</span>
            </label>
            <select
              value={dim}
              onChange={(e) => setDim(Number(e.target.value))}
              className="rounded-md border border-input bg-background px-3 py-2 text-sm text-foreground font-mono focus:outline-none focus:ring-1 focus:ring-ring appearance-none cursor-pointer"
            >
              {DIMS.map((d) => (
                <option key={d.value} value={d.value}>{d.label}</option>
              ))}
            </select>
          </div>

          <div className="flex flex-col gap-1">
            <label className="text-[10px] text-muted-foreground uppercase tracking-wider">Index</label>
            <div className="flex gap-2">
              {(["brute", "hnsw", "ivf"] as const).map((opt) => (
                <button
                  key={opt}
                  type="button"
                  onClick={() => setIndex(opt)}
                  className={`flex-1 rounded-md border px-3 py-1.5 text-xs transition-colors ${
                    index === opt
                      ? "border-[var(--v-accent)] bg-[var(--v-accent-muted)] text-foreground"
                      : "border-input bg-background text-muted-foreground hover:text-foreground"
                  }`}
                >
                  {opt === "brute" ? "Brute-force" : opt === "hnsw" ? "HNSW" : "IVF"}
                </button>
              ))}
            </div>
          </div>

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
