"use client";

import { use } from "react";
import dynamic from "next/dynamic";
import { OperationDetailView } from "@valori/studio";
import { LOCAL_CONNECTION_PROJECT_ID } from "@/lib/local-runtime/transport";

// Migrated to Shared Studio's OperationDetailView (Phase G2) — the
// Overview/Results/Proof/Metrics tabs are now the same implementation
// every host consumes. The Execution Explorer tab (graph visualization,
// depends on @xyflow/react — not a Studio dependency) is supplied via
// `renderExecution`, using Local's own existing ExecutionExplorer
// component unchanged. The old dual-mode
// components/operations/OperationDetailView.tsx stays in git history for
// rollback; it is no longer imported by this route.
//
// Phase L (performance): dynamic-imported, not a static import — @xyflow/react
// (~4 MB in node_modules, a meaningful chunk of minified JS) was previously
// bundled into this page's initial JS unconditionally, even though the
// Execution Explorer is one tab among several and most opens of an
// operation's detail page never click it.
const ExecutionExplorer = dynamic(() => import("@/components/operations/ExecutionExplorer"), {
  ssr: false,
  loading: () => <div className="p-6 text-sm text-muted-foreground">Loading execution graph…</div>,
});
export default function OperationDetailPage({ params }: { params: Promise<{ id: string }> }) {
  const { id } = use(params);
  return (
    <OperationDetailView
      projectId={LOCAL_CONNECTION_PROJECT_ID}
      operationId={id}
      backHref="/operations"
      renderExecution={(data, loading) => <ExecutionExplorer loading={loading} data={data} />}
    />
  );
}
