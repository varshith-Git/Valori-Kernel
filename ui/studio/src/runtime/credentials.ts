/**
 * Storage for a secret (an embedding/LLM provider API key) that a shared
 * hook needs to persist without knowing *how*. Desktop backs this with the
 * OS keychain (never writing the plaintext secret to disk); Cloud Web has
 * no such store and is expected to omit this from `StudioRuntime` entirely
 * — the hooks that use it fall back to plain `localStorage`, matching
 * Cloud's existing, documented behavior exactly (see useEmbeddingConfig.ts/
 * useLLMConfig.ts's own comments). This package does not change that
 * security posture in either direction — it only names the interface the
 * Desktop keychain adapter already implements today via `@/lib/native`.
 */
export interface CredentialStore {
  store(secret: string, existingRef?: string): Promise<string>;
  get(ref: string): Promise<string | null>;
  delete(ref: string): Promise<void>;
}
