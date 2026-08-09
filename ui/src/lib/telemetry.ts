// Desktop telemetry orchestration — Phase 1 (rfcs/desktop-telemetry, plan
// doc). Thin layer on top of native.ts's raw bridge functions
// (getTelemetryConsent/getInstallationId) and the Rust-side Tauri commands
// (get_app_info/enqueue_telemetry_event/check_and_clear_crash_marker,
// desktop/src-tauri/src/telemetry.rs). Every export here is a no-op outside
// the desktop shell and a no-op if the relevant consent toggle is off —
// callers never need their own consent-check branch.

import { nativeAvailable, getTelemetryConsent, getInstallationId } from "@/lib/native";

// No session id is generated or passed from here anymore — it's generated
// once in Rust (session_id() in telemetry.rs), initialized in setup() before
// either this JS event loop or a Rust-native call site (the background
// update check) can emit an event, and `enqueue_telemetry_event` stamps
// every envelope with it server-side. That's what fixes the old bug where
// JS and Rust each had their own, unrelated session id for the same launch.

// Which consent field gates an event — must match
// `desktop/src-tauri/src/telemetry.rs`'s `TelemetryCategory` exactly (the
// Rust side deserializes this as an internally-tagged enum using these
// literal snake_case values). "analytics" covers session lifecycle and
// startup timing; "crash" is only ever `studio_crashed`, gated on the
// independent `crash` consent toggle — see `reportSessionStarted` below,
// which is the one call site that passes `"crash"` explicitly.
type TelemetryCategory = "analytics" | "crash";

// Enqueues locally (studio.redb's telemetry_queue); the Rust-side
// background sender drains it on its own timer, re-checking this same
// category's consent immediately before every send — see telemetry.rs's
// module doc ("Consent boundary") for why the enqueue-time check here is
// not the only gate.
async function send(
  event: string,
  properties: Record<string, unknown> = {},
  category: TelemetryCategory = "analytics",
): Promise<void> {
  if (!nativeAvailable()) return;
  try {
    const { invoke } = await import("@tauri-apps/api/core");
    const installationId = await getInstallationId();
    await invoke("enqueue_telemetry_event", {
      event,
      properties,
      installationId,
      category,
    });
  } catch {
    // Best-effort — a telemetry failure must never be visible to the user
    // or affect anything real, same posture as every other fire-and-forget
    // write in this codebase (login_history, audit log).
  }
}

/** Call once, early in the app shell's lifecycle, after onboarding/consent
 *  state is known to be settled. Checks for a crash marker from the
 *  *previous* run first (see telemetry.rs's module doc for why crash
 *  reporting is split across two startups), then sends `session_started`
 *  with `previous_crashed` reflecting whether one was found — regardless of
 *  whether the crash report itself gets sent (that's gated on the `crash`
 *  toggle specifically, session_started is gated on `analytics`). */
export async function reportSessionStarted(startupTimeMs: number): Promise<void> {
  if (!nativeAvailable()) return;
  const consent = await getTelemetryConsent();

  let previousCrashed = false;
  try {
    const { invoke } = await import("@tauri-apps/api/core");
    const crash = await invoke<{
      panic_hash: string;
      panic_location: string;
      previous_session: string;
      uptime_before_crash_secs: number;
    } | null>("check_and_clear_crash_marker");
    if (crash) {
      previousCrashed = true;
      if (consent.crash) {
        // previous_session + uptime_before_crash_secs distinguish "crashed
        // 2s after launch" (startup bug) from "crashed after 4 hours" (slow
        // leak/edge case) — both came straight from the panic hook, not
        // reconstructed here.
        await send(
          "studio_crashed",
          {
            panic_hash: crash.panic_hash,
            panic_location: crash.panic_location,
            previous_session: crash.previous_session,
            uptime_before_crash_secs: crash.uptime_before_crash_secs,
          },
          "crash",
        );
      }
    }
  } catch {
    // Marker check failing is not itself worth reporting — see `send`'s
    // own best-effort posture above.
  }

  if (!consent.analytics) return;

  const info = await getAppInfo();
  await send("session_started", {
    startup_time_ms: startupTimeMs,
    previous_crashed: previousCrashed,
    version: info?.version,
    platform: info?.platform,
    arch: info?.arch,
  });
}

/** The startup waterfall (see startupMarks.ts) — one `startup_completed`
 *  event per launch, carrying whatever epoch-ms marks were actually
 *  captured (`rust_start_ms`, `react_mounted_ms`, `daemon_ready_ms`,
 *  `workspace_loaded_ms`, `interactive_ms`). Some may be legitimately
 *  absent (e.g. no `daemon_ready_ms` if neither call site fired this
 *  launch) — sent as-is, never backfilled with a guess. */
export async function reportStartupCompleted(marks: Record<string, number>): Promise<void> {
  const consent = await getTelemetryConsent();
  if (!consent.analytics) return;
  await send("startup_completed", marks);
}

/** Best-effort — called from a `beforeunload`/unmount handler, which in a
 *  desktop app is itself not fully reliable (a force-quit can skip it
 *  entirely). That's an accepted gap for Phase 1, not something worth
 *  building a heartbeat mechanism to close yet. */
export async function reportSessionEnded(): Promise<void> {
  const consent = await getTelemetryConsent();
  if (!consent.analytics) return;
  await send("session_ended", {});
}

export async function getAppInfo(): Promise<{ version: string; platform: string; arch: string } | null> {
  if (!nativeAvailable()) return null;
  try {
    const { invoke } = await import("@tauri-apps/api/core");
    return await invoke("get_app_info");
  } catch {
    return null;
  }
}
