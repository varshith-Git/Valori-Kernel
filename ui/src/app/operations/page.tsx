"use client";

import { OperationsExplorer } from "@valori/studio";
import { LOCAL_CONNECTION_PROJECT_ID } from "@/lib/local-runtime/transport";

export default function OperationsListPage() {
  return (
    <OperationsExplorer
      projectId={LOCAL_CONNECTION_PROJECT_ID}
      operationHref={(opId) => `/operations/${encodeURIComponent(opId)}`}
    />
  );
}
