"use client";

import { useState, useEffect } from "react";

function compute(iso?: string): string {
  if (!iso) return "never opened";
  const diff = Date.now() - new Date(iso).getTime();
  const mins = Math.floor(diff / 60_000);
  if (mins < 1)  return "just now";
  if (mins < 60) return `${mins}m ago`;
  const hrs = Math.floor(mins / 60);
  if (hrs < 24)  return `${hrs}h ago`;
  return `${Math.floor(hrs / 24)}d ago`;
}

/** Reactive relative-time string that refreshes every 60 s. */
export function useRelativeTime(iso?: string): string {
  const [label, setLabel] = useState(() => compute(iso));

  useEffect(() => {
    setLabel(compute(iso));
    const id = setInterval(() => setLabel(compute(iso)), 60_000);
    return () => clearInterval(id);
  }, [iso]);

  return label;
}
