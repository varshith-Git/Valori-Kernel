"use client";

import { useState, useEffect, useCallback } from "react";
import type { ToastPayload } from "@/lib/toast";

const DURATION_MS = 4500;

const STYLES: Record<string, string> = {
  error:   "bg-red-500/15 border-red-500/30 text-red-700",
  success: "bg-emerald-500/15 border-emerald-500/30 text-emerald-700",
  warning: "bg-amber-500/15 border-amber-500/30 text-amber-700",
};

const ICONS: Record<string, string> = {
  error: "✕",
  success: "✓",
  warning: "⚠",
};

export function Toaster() {
  const [toasts, setToasts] = useState<ToastPayload[]>([]);

  const dismiss = useCallback((id: string) => {
    setToasts((prev) => prev.filter((t) => t.id !== id));
  }, []);

  useEffect(() => {
    const handler = (e: Event) => {
      const { id, kind, message } = (e as CustomEvent<ToastPayload>).detail;
      setToasts((prev) => [...prev.slice(-3), { id, kind, message }]);
      setTimeout(() => dismiss(id), DURATION_MS);
    };
    window.addEventListener("valori:toast", handler);
    return () => window.removeEventListener("valori:toast", handler);
  }, [dismiss]);

  if (!toasts.length) return null;

  return (
    <div
      role="region"
      aria-live="polite"
      aria-label="Notifications"
      className="fixed bottom-5 right-5 z-50 flex flex-col gap-2 pointer-events-none"
    >
      {toasts.map((t) => (
        <div
          key={t.id}
          className={`flex items-start gap-3 rounded-xl border px-4 py-3 text-sm max-w-sm pointer-events-auto shadow-xl animate-in fade-in slide-in-from-bottom-2 duration-200 ${STYLES[t.kind]}`}
        >
          <span className="mt-px flex-shrink-0 font-bold text-xs opacity-80">{ICONS[t.kind]}</span>
          <span className="flex-1 leading-snug">{t.message}</span>
          <button
            onClick={() => dismiss(t.id)}
            aria-label="Dismiss"
            className="mt-px flex-shrink-0 opacity-50 hover:opacity-100 transition-opacity text-xs"
          >
            ✕
          </button>
        </div>
      ))}
    </div>
  );
}
