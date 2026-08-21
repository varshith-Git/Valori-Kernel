// Shared Valori Studio — public API.
//
// Hosts (Desktop Local, Desktop Cloud, Cloud Web) import only from here,
// never from internal file paths — everything below is the intentional
// surface; nothing else in src/ is meant to be imported directly.

// ── Runtime (interfaces the host must implement) ────────────────────────────
export type { Transport } from "./runtime/transport";
export type { CredentialStore } from "./runtime/credentials";
export type { StudioCapabilities } from "./runtime/capabilities";
export type { ProjectRef } from "./runtime/project";
export { StudioProvider, useTransport, useCapabilities, useCredentialStore } from "./runtime/context";
export type { StudioRuntime } from "./runtime/context";

// ── Navigation ───────────────────────────────────────────────────────────────
export { PROJECT_FEATURE_NAV } from "./navigation/projectNav";
export type { ProjectNavItem } from "./navigation/projectNav";

// ── Core product views ───────────────────────────────────────────────────────
export { ClusterView } from "./components/projects/ClusterView";
export { MetricsView } from "./components/projects/MetricsView";
export { SnapshotsView } from "./components/projects/SnapshotsView";
export { PlaygroundView } from "./components/projects/PlaygroundView";
export { GraphView } from "./components/graph/GraphView";
export { OperationsExplorer } from "./components/operations/OperationsExplorer";
export { OperationDetailView } from "./components/operations/OperationDetailView";
export { ProofView } from "./components/proof/ProofView";

// ── Tools ────────────────────────────────────────────────────────────────────
export { ToolsWorkspace } from "./components/tools/ToolsWorkspace";
export { MultiSearch } from "./components/collections/MultiSearch";
export { DocumentUploadTab } from "./components/collections/DocumentUploadTab";
export { DocumentsTab } from "./components/collections/DocumentsTab";
export { BulkInsertTab } from "./components/collections/BulkInsertTab";
export { TreeRagTab } from "./components/collections/TreeRagTab";
export { CommunityTab } from "./components/collections/CommunityTab";
export { EntityExtractionTab } from "./components/collections/EntityExtractionTab";
export { DiffTab } from "./components/collections/DiffTab";
export { ContradictionTab } from "./components/collections/ContradictionTab";
export { VerifyTab } from "./components/collections/VerifyTab";
export { CompliancePackTab } from "./components/collections/CompliancePackTab";
export { EvalTab } from "./components/collections/EvalTab";
export { CertifyTab } from "./components/collections/CertifyTab";
export { GdprTab } from "./components/collections/GdprTab";
export { VisualizeTab } from "./components/collections/VisualizeTab";
export { AskTab } from "./components/collections/AskTab";
export { TabShell } from "./components/collections/TabShell";

// ── Shared hooks ─────────────────────────────────────────────────────────────
export { useHealth } from "./lib/hooks/useHealth";
export { useCluster } from "./lib/hooks/useCluster";
export type { MemberView, ClusterStatusResponse } from "./lib/hooks/useCluster";
export { useProof } from "./lib/hooks/useProof";
export { useGraph, useNodeEdges } from "./lib/hooks/useGraph";
export type { GraphNode, GraphEdge, DocumentTree } from "./lib/hooks/useGraph";
export { useSearch } from "./lib/hooks/useSearch";
export type { SearchQuery, SearchState } from "./lib/hooks/useSearch";
export { useCollections } from "./lib/hooks/useCollections";
export type { CollectionRef } from "./lib/hooks/useCollections";
export {
  useEmbeddingConfig,
  PROVIDER_DEFAULTS,
  MODEL_DIMS,
  getModelDim,
  registerModelDims,
} from "./lib/hooks/useEmbeddingConfig";
export type { EmbeddingProvider, EmbeddingConfig } from "./lib/hooks/useEmbeddingConfig";
export { useLLMConfig, LLM_PROVIDER_DEFAULTS } from "./lib/hooks/useLLMConfig";
export type { LLMProvider, LLMConfig } from "./lib/hooks/useLLMConfig";

// ── Domain types ─────────────────────────────────────────────────────────────
export type * from "./types/valori";
export type { NsEvent, NsAuditResponse } from "./types/namespaceAudit";
