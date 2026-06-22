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

interface Props {
  project: string;
  open: boolean;
  onOpenChange: (o: boolean) => void;
  onCreate: (name: string) => Promise<void>;
}

const VALID = /^[a-zA-Z0-9_-]+$/;

export function CreateCollectionDialog({
  project,
  open,
  onOpenChange,
  onCreate,
}: Props) {
  const [name, setName] = useState("");
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState("");

  const submit = async () => {
    if (!VALID.test(name)) {
      setErr("Only letters, numbers, _ and - allowed");
      return;
    }
    setBusy(true);
    try {
      await onCreate(name.trim());
      setName("");
      setErr("");
      onOpenChange(false);
    } catch (e: unknown) {
      setErr(e instanceof Error ? e.message : "Create failed");
    } finally {
      setBusy(false);
    }
  };

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="bg-card border-border text-foreground max-w-md">
        <DialogHeader>
          <DialogTitle className="text-base">
            New collection in{" "}
            <span className="font-mono text-muted-foreground">{project}</span>
          </DialogTitle>
        </DialogHeader>
        <div className="py-2 flex flex-col gap-2">
          <Input
            autoFocus
            placeholder="collection-name"
            value={name}
            onChange={(e) => {
              setName(e.target.value);
              setErr("");
            }}
            onKeyDown={(e) => e.key === "Enter" && submit()}
            className="bg-accent border-input text-foreground placeholder:text-muted-foreground"
          />
          {name && (
            <p className="text-xs text-muted-foreground">
              Namespace:{" "}
              <code className="font-mono text-muted-foreground">
                {project}--{name}
              </code>
            </p>
          )}
          {err && <p className="text-xs text-red-400">{err}</p>}
        </div>
        <DialogFooter>
          <Button
            variant="outline"
            size="sm"
            onClick={() => onOpenChange(false)}
            className="border-input text-muted-foreground"
          >
            Cancel
          </Button>
          <Button
            size="sm"
            disabled={!name || busy}
            onClick={submit}
            className="bg-primary text-primary-foreground hover:bg-primary/90"
          >
            {busy ? "Creating…" : "Create collection"}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
