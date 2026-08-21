"use client";

import { ClusterView } from "@valori/studio";
import { LOCAL_CONNECTION_PROJECT_ID } from "@/lib/local-runtime/transport";

// Guidance shown when this node isn't part of a Raft cluster — the exact
// text the old standalone ClusterView showed for local mode, before its
// standalone-mode message moved behind Shared Studio's `standaloneHint`
// prop (see the collection-model / runtime-abstraction design docs).
const LOCAL_STANDALONE_HINT = (
  <>
    <p className="mt-2 text-xs text-muted-foreground max-w-sm mx-auto">
      This node is not part of a Raft cluster. To enable cluster mode,
      set <code className="font-mono bg-accent px-1 rounded">VALORI_CLUSTER_MEMBERS</code> and{" "}
      <code className="font-mono bg-accent px-1 rounded">VALORI_NODE_ID</code> and
      restart.
    </p>
    <pre className="mt-4 rounded-lg bg-background px-5 py-4 text-left text-xs text-accent-foreground font-mono inline-block">
{`docker compose -f docker-compose.cluster.yml up -d`}
    </pre>
  </>
);

export default function ClusterPage() {
  return (
    <ClusterView
      projectId={LOCAL_CONNECTION_PROJECT_ID}
      standaloneHint={LOCAL_STANDALONE_HINT}
    />
  );
}
