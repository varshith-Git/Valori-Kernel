"use client";

import { PlaygroundView } from "@valori/studio";
import { LOCAL_CONNECTION_PROJECT_ID } from "@/lib/local-runtime/transport";
import { resolveLocalCapabilities } from "@/lib/local-runtime/capabilities";

export default function PlaygroundPage() {
  return (
    <PlaygroundView
      projectId={LOCAL_CONNECTION_PROJECT_ID}
      capabilities={resolveLocalCapabilities()}
    />
  );
}
