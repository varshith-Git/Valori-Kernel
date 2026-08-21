import type { ProjectRef } from "./project";

/**
 * Turns "this project + this node-relative subpath" into whatever URL the
 * host actually needs to fetch. This is the one abstraction every shared
 * hook needs (confirmed by the atomic component/hook diff investigation —
 * every duplicated hook's only real difference was this URL prefix):
 *
 *   Desktop Local:  path(_, "/health")            -> "/api/health"
 *   Desktop Cloud:  path(id, "/health")            -> "/api/cloud/projects/{id}/health"
 *   Cloud Web:      path(id, "/health")            -> "/api/projects/{id}/health"
 *
 * `subpath` always starts with "/" and mirrors valori-node's own route
 * shape (e.g. "/health", "/namespaces", "/namespaces/{name}", "/search",
 * "/cluster", "/graph/nodes") — Shared Studio hooks never hardcode a host
 * route prefix themselves.
 */
export interface Transport {
  path(projectId: ProjectRef, subpath: string): string;
}
