"use client";

import { useEffect, useRef, useState } from "react";
import Link from "next/link";
import { Archive, HelpCircle, ScrollText, Settings } from "lucide-react";
import { ThemeToggle } from "@/components/layout/ThemeToggle";

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

  return (
    <div
      ref={popoverRef}
      style={{ position: "fixed", left: pos.left, bottom: pos.bottom, zIndex: 9999, width: "13rem" }}
      className="rounded-xl border border-border bg-card shadow-lg ring-1 ring-border/30 overflow-hidden"
    >
      <div className="flex items-center justify-between px-3 py-2.5">
        <span className="text-sm font-medium text-foreground">Appearance</span>
        <ThemeToggle />
      </div>
      <div className="mx-2 border-t border-border/60" />
      <div className="p-1.5 flex flex-col gap-0.5">
        <Link
          href="/logs"
          onClick={onClose}
          className="flex items-center gap-2.5 rounded-lg px-3 py-2 text-sm text-muted-foreground hover:bg-accent/70 hover:text-foreground transition-colors"
        >
          <ScrollText size={14} aria-hidden />
          Logs
        </Link>
        <Link
          href="/snapshots"
          onClick={onClose}
          className="flex items-center gap-2.5 rounded-lg px-3 py-2 text-sm text-muted-foreground hover:bg-accent/70 hover:text-foreground transition-colors"
        >
          <Archive size={14} aria-hidden />
          Snapshots
        </Link>
        <Link
          href="/help"
          onClick={onClose}
          className="flex items-center gap-2.5 rounded-lg px-3 py-2 text-sm text-muted-foreground hover:bg-accent/70 hover:text-foreground transition-colors"
        >
          <HelpCircle size={14} aria-hidden />
          Help &amp; docs
        </Link>
      </div>
      <div className="mx-2 border-t border-border/60" />
      <div className="p-1.5">
        <Link
          href="/settings"
          onClick={onClose}
          className="flex items-center gap-2.5 rounded-lg px-3 py-2 text-sm font-medium text-muted-foreground hover:bg-accent/70 hover:text-foreground transition-colors"
        >
          <Settings size={14} aria-hidden />
          All settings
        </Link>
      </div>
    </div>
  );
}
