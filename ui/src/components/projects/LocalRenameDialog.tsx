"use client";

import { useState } from "react";

export function LocalRenameDialog({ name, onClose, onSuccess }: {
  name: string;
  onClose: () => void;
  onSuccess: (newName: string) => void;
}) {
  const [value, setValue] = useState(name);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);

  async function submit(e: React.FormEvent) {
    e.preventDefault();
    const trimmed = value.trim();
    if (!trimmed) return;
    if (trimmed === name) { onClose(); return; }
    setLoading(true);
    const res = await fetch(`/api/projects/${encodeURIComponent(name)}`, {
      method: "PATCH",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ name: trimmed }),
    });
    if (!res.ok) {
      const d = await res.json().catch(() => ({})) as { error?: string };
      setError(d.error ?? "Rename failed");
      setLoading(false);
      return;
    }
    onSuccess(trimmed);
  }

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/60 p-4" onClick={onClose}>
      <form
        onSubmit={submit}
        onClick={(e) => e.stopPropagation()}
        className="w-full max-w-sm rounded-2xl border border-border bg-card p-6 space-y-4"
      >
        <h2 className="text-base font-semibold text-foreground">Rename project</h2>
        <input
          autoFocus
          autoCapitalize="none"
          autoCorrect="off"
          spellCheck={false}
          value={value}
          onChange={(e) => setValue(e.target.value)}
          className="w-full px-3 py-2 rounded-lg border border-border bg-background text-foreground text-sm focus:outline-none focus:ring-2 focus:ring-[var(--v-accent-ring)]"
        />
        {error && <p className="text-xs text-destructive">{error}</p>}
        <div className="flex gap-2 pt-1">
          <button type="button" onClick={onClose} disabled={loading} className="flex-1 px-4 py-2 border border-border text-foreground rounded-lg hover:bg-accent transition text-sm">Cancel</button>
          <button type="submit" disabled={loading || !value.trim()} className="flex-1 px-4 py-2 bg-primary text-primary-foreground font-semibold rounded-lg hover:opacity-90 transition disabled:opacity-50 text-sm">
            {loading ? "Saving…" : "Save"}
          </button>
        </div>
      </form>
    </div>
  );
}
