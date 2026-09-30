"use client";

/**
 * Client-side proof helpers shared across CertifyTab, GdprTab, and others.
 * Server-side counterpart lives in lib/server/proof.ts.
 */

/** Fetch the current BLAKE3 global state hash from the local /api/proof proxy. */
export async function fetchGlobalHash(): Promise<string | null> {
  try {
    const res = await fetch("/api/proof", { cache: "no-store" });
    if (!res.ok) return null;
    const d = (await res.json()) as { final_state_hash?: string };
    return d.final_state_hash ?? null;
  } catch {
    return null;
  }
}
