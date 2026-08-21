import type { CredentialStore } from "@valori/studio";
import {
  nativeAvailable,
  credentialStore,
  credentialGet,
  credentialDelete,
  migrateLegacyProviderCredential,
} from "@/lib/native";

// Wraps the existing Tauri OS-keychain bridge (@/lib/native) — Shared
// Studio never imports @/lib/native itself; this is the one file in the
// Desktop host that does. Behavior is unchanged from before Phase C: the
// package's useEmbeddingConfig/useLLMConfig still never write a plaintext
// apiKey to localStorage when a CredentialStore is present, still resolve
// the secret via `get`, still rotate/delete via `store`/`delete`.
export const localCredentialStore: CredentialStore = {
  store: (secret, existingRef) => credentialStore(secret, existingRef),
  get: (ref) => credentialGet(ref),
  delete: (ref) => credentialDelete(ref),
};

// `undefined` outside Tauri (e.g. `next dev` run standalone in a browser,
// not the desktop shell) — useEmbeddingConfig/useLLMConfig fall back to
// plain localStorage in that case, same as Cloud Web, which is the
// correct degrade: there is no OS keychain to reach.
export function resolveLocalCredentialStore(): CredentialStore | undefined {
  return nativeAvailable() ? localCredentialStore : undefined;
}

// Phase C's useEmbeddingConfig/useLLMConfig deliberately no longer call
// this themselves (it's a one-time Desktop-only upgrade path for
// installations that persisted a plaintext apiKey before the OS-keychain
// scheme existed — not a CredentialStore concern). Call once, host-side,
// before either hook's first read. Idempotent and a no-op outside Tauri.
export async function runLegacyCredentialMigration(): Promise<void> {
  if (!nativeAvailable()) return;
  await Promise.all([
    migrateLegacyProviderCredential("valori:embedding_config"),
    migrateLegacyProviderCredential("valori:llm_config"),
  ]);
}
