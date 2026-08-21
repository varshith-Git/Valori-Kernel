"use client";

import { useState } from "react";
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
  DialogFooter,
} from "@/components/ui/dialog";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { ChevronDown } from "lucide-react";

interface Props {
  project: string;
  existingCollections?: string[];
  open: boolean;
  onOpenChange: (o: boolean) => void;
  onCreate: (
    name: string,
    dim: number,
    index?: "brute" | "hnsw" | "ivf" | "bq" | "auto",
  ) => Promise<void>;
}

const VALID = /^[a-zA-Z0-9_-]+$/;

const INDEX_OPTIONS: { value: "brute" | "hnsw" | "ivf" | "bq" | "auto"; label: string }[] = [
  { value: "brute", label: "Brute (exact, default)" },
  { value: "auto",  label: "Auto (size-adaptive)" },
  { value: "hnsw",  label: "HNSW" },
  { value: "ivf",   label: "IVF" },
  { value: "bq",    label: "BQ (binary quantized)" },
];

export function CreateCollectionDialog({
  project,
  existingCollections = [],
  open,
  onOpenChange,
  onCreate,
}: Props) {
  const [name, setName]   = useState("");
  const [dim, setDim]     = useState<string>("");
  const [index, setIndex] = useState<"brute" | "hnsw" | "ivf" | "bq" | "auto">("brute");
  const [busy, setBusy]   = useState(false);
  const [err, setErr]     = useState("");

  const trimmed   = name.trim();
  // Phase 3.3: "default" has no special meaning — it's "already taken"
  // only if it's actually in `existingCollections`, exactly like any other
  // name. A brand-new project has no collections at all, "default"
  // included, so it must be creatable here like anything else.
  const alreadyExists = Boolean(
    trimmed &&
      existingCollections.some(
        (c) => c.toLowerCase() === trimmed.toLowerCase()
      )
  );
  const isValidFormat = trimmed.length > 0 && VALID.test(trimmed);

  const dimNum = parseInt(dim, 10);
  const dimValid = !Number.isNaN(dimNum) && dimNum >= 1 && dimNum <= 65535;

  const isValid = isValidFormat && !alreadyExists && dimValid;

  const reset = () => {
    setName("");
    setDim("");
    setIndex("brute");
    setErr("");
  };

  const submit = async () => {
    if (!VALID.test(trimmed)) {
      setErr("Only letters, numbers, _ and - allowed");
      return;
    }
    if (alreadyExists) {
      setErr(`Collection "${trimmed}" already exists. Choose another name.`);
      return;
    }
    if (!dimValid) {
      setErr("Dimension must be a whole number between 1 and 65535");
      return;
    }
    setBusy(true);
    try {
      await onCreate(trimmed, dimNum, index === "brute" ? undefined : index);
      reset();
      onOpenChange(false);
    } catch (e: unknown) {
      setErr(e instanceof Error ? e.message : "Create failed");
    } finally {
      setBusy(false);
    }
  };

  return (
    <Dialog open={open} onOpenChange={(o) => { if (!o) reset(); onOpenChange(o); }}>
      <DialogContent className="bg-card border-border text-foreground max-w-md">
        <DialogHeader>
          <DialogTitle className="text-base">
            New collection in{" "}
            <span className="font-mono text-muted-foreground">{project}</span>
          </DialogTitle>
        </DialogHeader>

        <div className="py-2 flex flex-col gap-3">
          {/* Name */}
          <div className="flex flex-col gap-1">
            <label className="text-xs font-medium text-muted-foreground">Name</label>
            <Input
              autoFocus
              autoCapitalize="none"
              autoCorrect="off"
              spellCheck={false}
              placeholder="collection-name"
              value={name}
              onChange={(e) => { setName(e.target.value); setErr(""); }}
              onKeyDown={(e) => e.key === "Enter" && isValid && !busy && submit()}
              className={`bg-accent text-foreground placeholder:text-muted-foreground ${
                trimmed && !isValidFormat
                  ? "border-red-500 focus-visible:ring-red-500/30"
                  : alreadyExists
                  ? "border-amber-500 focus-visible:ring-amber-500/30"
                  : "border-input"
              }`}
            />
            <p className="text-[11px] text-muted-foreground">
              Letters, numbers, <code className="font-mono">_</code> and{" "}
              <code className="font-mono">-</code> only · cannot be changed later
            </p>
            {trimmed && !isValidFormat && (
              <p className="text-xs text-red-500">Invalid character — use only a–z, 0–9, _ or -</p>
            )}
            {alreadyExists && (
              <p className="text-xs text-amber-500 font-medium">
                {`Collection "${trimmed}" already exists. Please choose a different name.`}
              </p>
            )}
          </div>

          {/* Dimension */}
          <div className="flex flex-col gap-1">
            <label className="text-xs font-medium text-muted-foreground">
              Dimension <span className="text-red-400">*</span>
            </label>
            <Input
              type="number"
              min={1}
              max={65535}
              placeholder="e.g. 768"
              value={dim}
              onChange={(e) => { setDim(e.target.value); setErr(""); }}
              onKeyDown={(e) => e.key === "Enter" && isValid && !busy && submit()}
              className={`bg-accent text-foreground placeholder:text-muted-foreground ${
                dim && !dimValid
                  ? "border-red-500 focus-visible:ring-red-500/30"
                  : "border-input"
              }`}
            />
            <p className="text-[11px] text-muted-foreground">
              Must match your embedding model · immutable after creation
            </p>
            {dim && !dimValid && (
              <p className="text-xs text-red-500">Must be a whole number between 1 and 65535</p>
            )}
          </div>

          {/* Metric */}
          <div className="flex flex-col gap-1">
            <label className="text-xs font-medium text-muted-foreground">
              Metric <span className="text-red-400">*</span>
            </label>
            <div className="relative">
              <select
                disabled
                value="squared_l2"
                className="w-full appearance-none rounded-lg border border-input bg-accent/50 px-3 py-2 pr-8 text-sm text-muted-foreground cursor-not-allowed"
              >
                <option value="squared_l2">Squared L2</option>
              </select>
            </div>
            <p className="text-[11px] text-muted-foreground">
              Deterministically computed in Q16.16 fixed-point math
            </p>
          </div>

          {/* Index */}
          <div className="flex flex-col gap-1">
            <label className="text-xs font-medium text-muted-foreground">Index</label>
            <div className="relative">
              <select
                value={index}
                onChange={(e) => setIndex(e.target.value as typeof index)}
                className="w-full appearance-none rounded-lg border border-input bg-accent px-3 py-2 pr-8 text-sm text-foreground focus:outline-none focus:ring-1 focus:ring-ring cursor-pointer"
              >
                {INDEX_OPTIONS.map((opt) => (
                  <option key={opt.value} value={opt.value}>{opt.label}</option>
                ))}
              </select>
              <ChevronDown size={13} className="pointer-events-none absolute right-3 top-1/2 -translate-y-1/2 text-muted-foreground" />
            </div>
          </div>

          {err && <p className="text-xs text-red-400">{err}</p>}
        </div>

        <DialogFooter>
          <Button
            variant="outline"
            size="sm"
            onClick={() => { reset(); onOpenChange(false); }}
            className="border-input text-muted-foreground"
          >
            Cancel
          </Button>
          <Button
            size="sm"
            disabled={!isValid || busy}
            onClick={submit}
            className="bg-primary text-primary-foreground hover:bg-primary/90 disabled:opacity-40"
          >
            {busy ? "Creating…" : "Create collection"}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
