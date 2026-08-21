// Phase C standalone verification only — NOT a permanent demo app, NOT
// exported from src/index.ts, NOT wired into any host. Its only job is to
// prove, at compile time, that a completely mocked host (no Tauri, no
// Supabase, no Next.js) can construct every headline shared component from
// this package's own public API. If this file typechecks, the package's
// props/exports are genuinely consumable by an outside caller — not just
// internally self-consistent.

import {
  StudioProvider,
  MetricsView,
  ClusterView,
  ProofView,
  ToolsWorkspace,
  type Transport,
  type CredentialStore,
  type StudioCapabilities,
} from "../index";

// A host implements Transport by deciding its own URL scheme — this mock
// stands in for LocalRuntime/CloudRuntime without importing either.
const mockTransport: Transport = {
  path: (projectId, subpath) => `/mock/${projectId}${subpath}`,
};

const mockCredentials: CredentialStore = {
  store: async (secret) => `ref-${secret.length}`,
  get: async () => null,
  delete: async () => {},
};

const mockCapabilities: StudioCapabilities = {
  localFilesystem: false,
  multiCollectionPicker: false,
  osKeychain: false,
};

export function ExampleHostTree() {
  const projectId = "demo-project";

  return (
    <StudioProvider
      runtime={{ transport: mockTransport, credentials: mockCredentials, capabilities: mockCapabilities }}
    >
      <MetricsView projectId={projectId} capabilities={mockCapabilities} />
      <ClusterView projectId={projectId} />
      <ProofView projectId={projectId} nodeUrl="https://example.invalid" />
      <ToolsWorkspace projectId={projectId} projectName="Demo" />
    </StudioProvider>
  );
}
