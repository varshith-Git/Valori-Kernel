"use client";

import { StudioProvider } from "@valori/studio";
import { cloudTransport } from "./transport";
import { resolveCloudCredentialStore } from "./credentials";
import { resolveCloudCapabilities } from "./capabilities";

// Mounted per-page around a migrated Shared Studio feature (not at the
// /cloud layout root — Cloud auth/project-authorization already happens
// server-side in ProjectLayout/each page before this ever renders, and
// this provider carries no auth state of its own, so there is no benefit
// to widening its scope beyond the feature that actually needs it).
export function CloudStudioProvider({ children }: { children: React.ReactNode }) {
  return (
    <StudioProvider
      runtime={{
        transport: cloudTransport,
        credentials: resolveCloudCredentialStore(),
        capabilities: resolveCloudCapabilities(),
      }}
    >
      {children}
    </StudioProvider>
  );
}
