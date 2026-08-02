"use client";

import { useEffect, useRef } from "react";
import Link from "next/link";
import { Archive, HelpCircle, ScrollText, Settings, Sun, Moon } from "lucide-react";
import { useTheme } from "@/lib/theme";
import { cn } from "@/lib/utils";

export type PopoverPos = { left: number; bottom: number };

export function SettingsPopover({
  open,
  onClose,
  pos,
}: {
  open: boolean;
  onClose: () => void;
  pos: PopoverPos | null;
}) {
  const { pref, setTheme } = useTheme();
  const popoverRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!open) return;
    function onClickOutside(e: MouseEvent) {
      if (popoverRef.current && !popoverRef.current.contains(e.target as Node)) {
        onClose();
      }
    }
    function onEsc(e: KeyboardEvent) {
      if (e.key === "Escape") onClose();
    }
    document.addEventListener("mousedown", onClickOutside);
    document.addEventListener("keydown", onEsc);
    return () => {
      document.removeEventListener("mousedown", onClickOutside);
      document.removeEventListener("keydown", onEsc);
    };
  }, [open, onClose]);

  if (!open || !pos) return null;

  const openSettings = () => {
    onClose();
    window.dispatchEvent(new CustomEvent("valori:open-settings"));
  };

  return (
    <div
      ref={popoverRef}
      style={{ position: "fixed", left: pos.left, bottom: pos.bottom, zIndex: 9999, width: "14rem" }}
      className="rounded-xl border border-border bg-card shadow-lg ring-1 ring-border/30 overflow-hidden"
    >
      {/* Appearance */}
      <div className="p-2.5">
        <p className="text-[10px] font-semibold uppercase tracking-widest text-muted-foreground px-1.5 mb-1.5">
          Appearance
        </p>
        <div className="grid grid-cols-2 gap-1">
          <button
            onClick={() => setTheme("light")}
            className={cn(
              "flex items-center gap-1.5 rounded-lg px-2.5 py-1.5 text-xs font-medium transition-colors",
              pref === "light"
                ? "bg-[var(--v-accent-muted)] text-[var(--v-accent)] border border-[var(--v-accent)]/30"
                : "text-muted-foreground hover:bg-accent/70 hover:text-foreground border border-transparent",
            )}
          >
            <Sun size={12} aria-hidden />
            Light
          </button>
          <button
            onClick={() => setTheme("dark")}
            className={cn(
              "flex items-center gap-1.5 rounded-lg px-2.5 py-1.5 text-xs font-medium transition-colors",
              pref === "dark"
                ? "bg-[var(--v-accent-muted)] text-[var(--v-accent)] border border-[var(--v-accent)]/30"
                : "text-muted-foreground hover:bg-accent/70 hover:text-foreground border border-transparent",
            )}
          >
            <Moon size={12} aria-hidden />
            Dark
          </button>
        </div>
      </div>

      <div className="mx-2 border-t border-border/60" />

      {/* Navigation links */}
      <div className="p-1.5 flex flex-col gap-0.5">
        <Link
          href="/logs"
          onClick={onClose}
          className="flex items-center gap-2.5 rounded-lg px-3 py-2 text-sm text-muted-foreground hover:bg-accent/70 hover:text-foreground transition-colors"
        >
          <ScrollText size={13} aria-hidden />
          Logs
        </Link>
        <Link
          href="/snapshots"
          onClick={onClose}
          className="flex items-center gap-2.5 rounded-lg px-3 py-2 text-sm text-muted-foreground hover:bg-accent/70 hover:text-foreground transition-colors"
        >
          <Archive size={13} aria-hidden />
          Snapshots
        </Link>
        <Link
          href="/help"
          onClick={onClose}
          className="flex items-center gap-2.5 rounded-lg px-3 py-2 text-sm text-muted-foreground hover:bg-accent/70 hover:text-foreground transition-colors"
        >
          <HelpCircle size={13} aria-hidden />
          Help &amp; docs
        </Link>
      </div>

      <div className="mx-2 border-t border-border/60" />

      {/* All settings */}
      <div className="p-1.5">
        <button
          onClick={openSettings}
          className="w-full flex items-center gap-2.5 rounded-lg px-3 py-2 text-sm font-medium text-muted-foreground hover:bg-accent/70 hover:text-foreground transition-colors"
        >
          <Settings size={13} aria-hidden />
          All settings
          <span className="ml-auto text-[10px] text-muted-foreground/60 font-mono">⌘,</span>
        </button>
      </div>
    </div>
  );
}
