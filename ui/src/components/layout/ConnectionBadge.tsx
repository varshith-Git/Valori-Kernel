"use client";

import { useHealth } from "@/lib/hooks/useHealth";
import { useCluster } from "@/lib/hooks/useCluster";

export function ConnectionBadge() {
  const { online, status } = useHealth();
  const { isStandalone, isLeader, members, nodeId } = useCluster();

  if (!online) {
    return (
      <span className="flex items-center gap-1.5 text-xs text-red-400">
        <span className="h-2 w-2 rounded-full bg-red-400 animate-pulse" />
        unreachable
      </span>
    );
  }

  const dotColor =
    status === "ok" ? "bg-emerald-400"
    : status === "degraded" ? "bg-amber-400"
    : "bg-red-400";

  const textColor =
    status === "ok" ? "text-emerald-400"
    : status === "degraded" ? "text-amber-400"
    : "text-red-400";

  if (!isStandalone && members.length > 0) {
    return (
      <span className={`flex items-center gap-1.5 text-xs ${textColor}`}>
        <span className={`h-2 w-2 rounded-full ${dotColor}`} />
        {isLeader ? "leader" : "follower"}
        <span className="text-muted-foreground">·</span>
        <span className="text-muted-foreground">node-{nodeId} · {members.length} nodes</span>
      </span>
    );
  }

  return (
    <span className={`flex items-center gap-1.5 text-xs ${textColor}`}>
      <span className={`h-2 w-2 rounded-full ${dotColor}`} />
      {status ?? "connected"}
      <span className="text-muted-foreground">·</span>
      <span className="text-muted-foreground">standalone</span>
    </span>
  );
}
