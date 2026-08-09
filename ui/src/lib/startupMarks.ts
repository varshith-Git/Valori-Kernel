// Startup waterfall (desktop telemetry Phase 2, "startup breakdown" —
// rfcs/desktop-telemetry). `startup_time_ms` on `session_started` is a
// single number; this captures where the time actually goes: Rust process
// start, React mount, daemon-ready, workspace-loaded, interactive. Those
// five moments happen in different files (and two different processes —
// Rust and this JS runtime) with no natural way to hand data directly to
// each other, so this module is the one shared meeting point.
//
// All marks are wall-clock epoch ms (`Date.now()` / the Rust side's
// `SystemTime::now()`), not `performance.now()` — a per-process/page
// monotonic clock can't be compared across the Rust/JS boundary, but two
// system clocks on the same machine can be diffed safely.
//
// The five phases are NOT a strict sequential waterfall today — the daemon
// is started from JS, fire-and-forget, concurrently with React mounting,
// not gated before it (see AppShellGate.tsx / Welcome.tsx). Each mark is
// still meaningful on its own (delta from `rust_start_ms`), just not
// guaranteed to arrive in the listed order.

import { nativeAvailable } from "@/lib/native";
import { reportStartupCompleted } from "@/lib/telemetry";

export type RecordableMark = "react_mounted_ms" | "daemon_ready_ms" | "workspace_loaded_ms" | "interactive_ms";

const marks: Partial<Record<RecordableMark, number>> = {};
let sent = false;

/** Record a mark now, if this launch hasn't already recorded it. Recording
 *  `interactive_ms` — the last phase in practice — triggers sending the
 *  `startup_completed` event exactly once. */
export function markStartupPhase(name: RecordableMark): void {
  if (name in marks) return;
  marks[name] = Date.now();
  if (name === "interactive_ms") finalize();
}

// rust_start_ms comes from a Tauri command (Rust's own SystemTime::now(),
// captured as the first line of run()), not from Date.now() here — fetched
// once, lazily.
let rustStartPromise: Promise<number | null> | null = null;
function fetchRustStartMs(): Promise<number | null> {
  if (!nativeAvailable()) return Promise.resolve(null);
  if (!rustStartPromise) {
    rustStartPromise = (async () => {
      try {
        const { invoke } = await import("@tauri-apps/api/core");
        return await invoke<number>("get_rust_start_ms");
      } catch {
        return null;
      }
    })();
  }
  return rustStartPromise;
}

function finalize(): void {
  if (sent) return;
  sent = true;
  fetchRustStartMs()
    .then((rustStartMs) => {
      const payload: Partial<Record<"rust_start_ms" | RecordableMark, number>> = { ...marks };
      if (rustStartMs != null) payload.rust_start_ms = rustStartMs;
      return reportStartupCompleted(payload);
    })
    .catch(() => {
      // Best-effort — same posture as every other telemetry send in this
      // codebase (see telemetry.ts's `send`).
    });
}
