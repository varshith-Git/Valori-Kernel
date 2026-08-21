"use client";

import { useEffect, useRef } from "react";
import { StudioProvider } from "@valori/studio";
import { localTransport } from "./transport";
import { resolveLocalCredentialStore, runLegacyCredentialMigration } from "./credentials";
import { resolveLocalCapabilities } from "./capabilities";

// Desktop Local's runtime wiring for @valori/studio — placed around
// AppShellGate's desktop shell branch (see AppShellGate.tsx), which is
// already exactly "Local product routes": /cloud/* and the auth pages
// (login/signup/forgot-password) are exempted upstream of this component,
// so this never wraps anything Cloud- or auth-specific.
export function LocalStudioProvider({ children }: { children: React.ReactNode }) {
  const migrated = useRef(false);

  useEffect(() => {
    if (migrated.current) return;
    migrated.current = true;
    runLegacyCredentialMigration().catch((e) => {
      console.error("legacy credential migration failed:", e);
    });
  }, []);

  return (
    <StudioProvider
      runtime={{
        transport: localTransport,
        credentials: resolveLocalCredentialStore(),
        capabilities: resolveLocalCapabilities(),
      }}
    >
      {children}
    </StudioProvider>
  );
}
