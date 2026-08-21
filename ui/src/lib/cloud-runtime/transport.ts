import type { Transport } from "@valori/studio";

// Desktop Cloud's own runtime — deliberately NOT LocalRuntime. Every Cloud
// project has its own Kernel Cloud API route under
// /api/cloud/projects/[id]/*, which proxies to that project's real
// provisioned node (see src/lib/server/nodeProxy.ts). Shared Studio itself
// never sees this prefix — it only ever calls transport.path(projectId,
// subpath) and gets back whatever URL the host decides.
export const cloudTransport: Transport = {
  path: (projectId, subpath) => `/api/cloud/projects/${projectId}${subpath}`,
};
