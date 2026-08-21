"use client";

import { createContext, useContext, type ReactNode } from "react";
import type { Transport } from "./transport";
import type { CredentialStore } from "./credentials";
import type { StudioCapabilities } from "./capabilities";

/** Everything a host must supply before mounting any Shared Studio feature.
 *  `credentials` is optional — omit it on Cloud Web, where the credential
 *  hooks fall back to plain localStorage (see credentials.ts). */
export interface StudioRuntime {
  transport: Transport;
  capabilities: StudioCapabilities;
  credentials?: CredentialStore;
}

const StudioContext = createContext<StudioRuntime | null>(null);

/** Wrap a host's project tree once with the runtime it resolved (LocalRuntime
 *  or CloudRuntime) — every Shared Studio hook/component reads it from here
 *  instead of importing Tauri/Supabase/Next.js directly. */
export function StudioProvider({
  runtime,
  children,
}: {
  runtime: StudioRuntime;
  children: ReactNode;
}) {
  return <StudioContext.Provider value={runtime}>{children}</StudioContext.Provider>;
}

function useStudioRuntime(): StudioRuntime {
  const ctx = useContext(StudioContext);
  if (!ctx) {
    throw new Error(
      "Shared Studio hooks/components must be rendered inside a <StudioProvider runtime={...}>."
    );
  }
  return ctx;
}

export function useTransport(): Transport {
  return useStudioRuntime().transport;
}

export function useCapabilities(): StudioCapabilities {
  return useStudioRuntime().capabilities;
}

/** `undefined` on any host that didn't supply one (Cloud Web) — callers must
 *  handle that case themselves (see useEmbeddingConfig.ts/useLLMConfig.ts). */
export function useCredentialStore(): CredentialStore | undefined {
  return useStudioRuntime().credentials;
}
