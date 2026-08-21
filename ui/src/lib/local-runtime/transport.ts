import type { Transport } from "@valori/studio";

// Desktop Local's Transport. Local has exactly one active connection at a
// time (see connection.ts's getApiUrl()/setApiUrl() — opening a project
// rewrites this single pointer to that project's own dedicated daemon
// port; see docs/architecture — the Desktop project/daemon model
// investigation). There is no scenario where the currently-mounted UI
// needs to pick between several simultaneously-reachable local nodes, so
// `projectId` is accepted (Shared Studio always passes one) but genuinely
// unused here — every subpath resolves through the SAME existing local
// `/api/*` proxy routes, which already resolve to whichever node is
// currently connected. This is intentional, not an oversight: Shared
// Studio never needs to know that.
export const localTransport: Transport = {
  path: (_projectId, subpath) => `/api${subpath}`,
};

// A stable, meaningless placeholder passed as `projectId` to every
// Shared Studio component mounted on a route with no per-project URL
// segment (/metrics, /cluster, /operations, /playground, ...) — Local's
// single-connection model has no real per-request project identity to
// give it, and `localTransport.path()` above ignores this value entirely.
// (Routes that DO have a real project name in their URL, e.g.
// /projects/[name]/..., should pass that name instead — see the
// collection-route migration notes.)
export const LOCAL_CONNECTION_PROJECT_ID = "local";
