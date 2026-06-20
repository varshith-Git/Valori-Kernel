"use client";

import { useHealth } from "@/lib/hooks/useHealth";

export function ConnectionBadge() {
  const { online, status } = useHealth();

  if (!online) {
    return (
      <span className="flex items-center gap-1.5 text-xs text-red-400">
        <span className="h-2 w-2 rounded-full bg-red-400 animate-pulse" />
        unreachable
      </span>
    );
  }

  const color =
    status === "ok" ? "bg-emerald-400 text-emerald-400"
    : status === "degraded" ? "bg-amber-400 text-amber-400"
    : "bg-red-400 text-red-400";

  return (
    <span className={`flex items-center gap-1.5 text-xs ${color.split(" ")[1]}`}>
      <span className={`h-2 w-2 rounded-full ${color.split(" ")[0]}`} />
      {status ?? "connected"}
    </span>
  );
}
