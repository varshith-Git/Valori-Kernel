/**
 * What the current runtime can offer — a fact about the environment, never
 * an authorization decision (that is enforced entirely by the host, before
 * a project ever reaches Shared Studio; see the runtime abstraction design
 * doc). Deliberately narrow: each flag corresponds to one concrete UI
 * branch that already existed before extraction, not a speculative one.
 */
export interface StudioCapabilities {
  /** Gates MetricsView's WAL/snapshot-file info cards — true only where the
   *  browser and the node share a filesystem (Desktop Local). */
  localFilesystem: boolean;
  /** Gates PlaygroundView's collection picker — true where a runtime has
   *  more than one collection worth switching between via that picker. */
  multiCollectionPicker: boolean;
  /** True when a CredentialStore backed by something more durable than
   *  localStorage (the OS keychain) is available — informational; no
   *  component branches on this directly today. */
  osKeychain: boolean;
  /** Gates DocumentUploadTab's client-driven fallback fields (embedding
   *  provider/model/apiKey/endpoint/chunk config, contextual enrichment) —
   *  true only where a host's own ingest route can fall back to its own
   *  chunk+embed+insert pipeline when the node has no server embed
   *  provider configured (Desktop Local today). Optional, defaulting to
   *  off, so existing 0.1.0-shaped capabilities objects (which don't set
   *  this field at all) keep their current behavior unchanged. */
  clientEmbeddingFallback?: boolean;
}
