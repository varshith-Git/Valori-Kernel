"use client";

import { use, useState } from "react";
import { ToolsWorkspace, GraphView, useCollections } from "@valori/studio";
import { LOCAL_CONNECTION_PROJECT_ID } from "@/lib/local-runtime/transport";
import { resolveLocalCapabilities } from "@/lib/local-runtime/capabilities";

// Migrated to Shared Studio's ToolsWorkspace (Phase G2) — the 16 tabs this
// page used to render itself are now the same implementation every host
// consumes. Graph is wired in via `extraTabs`, using the same GraphView
// (embedded mode) the standalone /graph-equivalent pages use elsewhere —
// not a second graph implementation. The old inline 18-tab
// CollectionPage/CollectionHeader/CollectionInfo implementation stays in
// git history for rollback; it is no longer imported by this route.
export default function CollectionPage({
  params,
}: {
  params: Promise<{ name: string; collection: string }>;
}) {
  const { name, collection } = use(params);
  const project = decodeURIComponent(name);
  const col = decodeURIComponent(collection);

  // Tracks the currently selected collection (canonical name) — starts
  // from the URL, but ToolsWorkspace owns collection switching internally
  // (its own picker), so this must follow via onCollectionChange rather
  // than staying frozen at whatever the URL said on first render. The
  // Graph extraTab below needs the up-to-date value, same as every other
  // tab ToolsWorkspace renders itself.
  const [selected, setSelected] = useState(col);

  // `selected` is the canonical collection name — resolve it to whatever
  // the node actually calls it: bare for a new-style collection,
  // "${project}--col" for one created before this reconciliation.
  const { raw } = useCollections(LOCAL_CONNECTION_PROJECT_ID);
  const namespace = raw.find((r) => r.name === selected)?.rawNamespace ?? selected;

  return (
    <ToolsWorkspace
      projectId={LOCAL_CONNECTION_PROJECT_ID}
      projectName={project}
      initialCollection={col}
      onCollectionChange={setSelected}
      capabilities={resolveLocalCapabilities()}
      settingsHref="/settings"
      extraTabs={[
        {
          value: "graph",
          label: "Graph",
          tip: "Visualise Document→Chunk relationships and entity links",
          group: "analyze",
          render: () => (
            <GraphView projectId={LOCAL_CONNECTION_PROJECT_ID} namespace={namespace} embedded />
          ),
        },
      ]}
    />
  );
}
