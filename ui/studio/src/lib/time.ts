/**
 * Shared time-formatting utilities.
 * All functions are pure (no React, no side-effects) so they can be used
 * in both server and client modules.
 */

/** Returns a human-readable "X ago" string for an ISO-8601 timestamp. */
export function timeAgo(iso: string): string {
  const secs = Math.floor((Date.now() - new Date(iso).getTime()) / 1000);
  if (secs < 60) return `${secs}s ago`;
  if (secs < 3600) return `${Math.floor(secs / 60)}m ago`;
  if (secs < 86400) return `${Math.floor(secs / 3600)}h ago`;
  return `${Math.floor(secs / 86400)}d ago`;
}
