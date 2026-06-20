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

  const ns = `${project}--${collection}`;

  const submit = async () => {
    setBusy(true);
    try {
      await onDelete();
      setConfirm("");
      onOpenChange(false);
    } finally {
      setBusy(false);
    }
  };

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="bg-zinc-900 border-zinc-800 text-white max-w-md">
        <DialogHeader>
          <DialogTitle className="text-base">Delete collection</DialogTitle>
        </DialogHeader>
        <div className="py-2 flex flex-col gap-3">
          <p className="text-sm text-zinc-400">
            This permanently deletes{" "}
            <code className="font-mono text-zinc-300">{ns}</code> and all its
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
            className="bg-zinc-800 border-zinc-700 text-white placeholder:text-zinc-600"
          />
        </div>
        <DialogFooter>
          <Button
            variant="outline"
            size="sm"
            onClick={() => onOpenChange(false)}
            className="border-zinc-700 text-zinc-400"
          >
            Cancel
          </Button>
          <Button
            size="sm"
            disabled={confirm !== collection || busy}
            onClick={submit}
            className="bg-red-600 text-white hover:bg-red-700 disabled:opacity-40"
          >
            {busy ? "Deleting…" : "Delete collection"}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
