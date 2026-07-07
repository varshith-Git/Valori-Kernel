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
  collection: string;
  open: boolean;
  onOpenChange: (o: boolean) => void;
  onDelete: () => Promise<void>;
}

export function DeleteCollectionDialog({
  project,
  collection,
  open,
  onOpenChange,
  onDelete,
}: Props) {
  const [confirm, setConfirm] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");

  const ns = `${project}--${collection}`;

  const submit = async () => {
    setBusy(true);
    setError("");
    try {
      await onDelete();
      setConfirm("");
      onOpenChange(false);
    } catch {
      setError("Failed to delete collection");
    } finally {
      setBusy(false);
    }
  };

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="bg-card border-border text-foreground max-w-md">
        <DialogHeader>
          <DialogTitle className="text-base">Delete collection</DialogTitle>
        </DialogHeader>
        <div className="py-2 flex flex-col gap-3">
          <p className="text-sm text-muted-foreground">
            This permanently deletes{" "}
            <code className="font-mono text-accent-foreground">{ns}</code> and all its
            vectors. Type the collection name to confirm.
          </p>
          <Input
            autoFocus
            placeholder={collection}
            value={confirm}
            onChange={(e) => setConfirm(e.target.value)}
            onKeyDown={(e) =>
              e.key === "Enter" && confirm === collection && submit()
            }
            className="bg-accent border-input text-foreground placeholder:text-muted-foreground"
          />
          {error && <p className="text-xs text-red-700">{error}</p>}
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
            disabled={confirm !== collection || busy}
            onClick={submit}
            className="bg-red-600 text-foreground hover:bg-red-700 disabled:opacity-40"
          >
            {busy ? "Deleting…" : "Delete collection"}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
