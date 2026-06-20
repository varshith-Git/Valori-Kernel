"use client";

import { useHealth } from "@/lib/hooks/useHealth";

export function ConnectionBadge() {
  const { online, role, nodeId, status } = useHealth();

  if (!online) {
    return (
      <span className="flex items-center gap-1.5 text-xs text-red-400">
        <span className="h-2 w-2 rounded-full bg-red-400 animate-pulse" />
        unreachable
      </span>
    );
  }

  return (
    <span className="flex items-center gap-1.5 text-xs text-emerald-400">
      <span className="h-2 w-2 rounded-full bg-emerald-400" />
      {status === "ok" ? "connected" : status}
      {nodeId != null && ` · node-${nodeId}`}
      {role && ` · ${role}`}
    </span>
  );
}
