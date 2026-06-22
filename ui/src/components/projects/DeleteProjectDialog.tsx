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
  name: string;
  open: boolean;
  onClose: () => void;
  onDelete: () => Promise<void>;
}

export function DeleteProjectDialog({ name, open, onClose, onDelete }: Props) {
  const [confirm, setConfirm] = useState("");
  const [busy, setBusy] = useState(false);

  const submit = async () => {
    if (confirm !== name) return;
    setBusy(true);
    try {
      await onDelete();
      setConfirm("");
      onClose();
    } finally {
      setBusy(false);
    }
  };

  return (
    <Dialog open={open} onOpenChange={(o) => { if (!o) { setConfirm(""); onClose(); } }}>
      <DialogContent className="bg-card border-input max-w-sm">
        <DialogHeader>
          <DialogTitle className="text-foreground text-base">Delete Project</DialogTitle>
        </DialogHeader>
        <div className="flex flex-col gap-3 pt-1">
          <p className="text-xs text-muted-foreground">
            This permanently drops the{" "}
            <code className="font-mono text-card-foreground">{name}</code> namespace and
            all its vectors. Type the project name to confirm.
          </p>
          <input
            autoFocus
            value={confirm}
            onChange={(e) => setConfirm(e.target.value)}
            onKeyDown={(e) => e.key === "Enter" && submit()}
            placeholder={name}
            className="rounded-md border border-input bg-background px-3 py-2 text-sm text-foreground placeholder:text-muted-foreground focus:outline-none focus:ring-1 focus:ring-red-700"
          />
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
              disabled={busy || confirm !== name}
              className="bg-red-700 text-foreground hover:bg-red-600 disabled:opacity-40"
            >
              {busy ? "Deleting…" : "Delete"}
            </Button>
          </div>
        </div>
      </DialogContent>
    </Dialog>
  );
}
