import type { CredentialStore } from "@valori/studio";
import { nativeAvailable, credentialStore, credentialGet, credentialDelete } from "@/lib/native";

// Desktop Cloud is still the Desktop/Tauri application — provider
// credentials (embedding API keys etc.) still belong in the real OS
// keychain, not in a Cloud-hosted secret store. Same native bridge
// LocalRuntime uses (crates/valori-ffi / Tauri's OS keychain plugin), no
// separate implementation — but this is its own CloudRuntime object, not a
// re-export of LocalRuntime's, per the explicit "do not reuse LocalRuntime"
// instruction: Cloud and Local are allowed to diverge independently even
// though today they happen to resolve to the same native calls.
export const cloudCredentialStore: CredentialStore = {
  store: (secret, existingRef) => credentialStore(secret, existingRef),
  get: (ref) => credentialGet(ref),
  delete: (ref) => credentialDelete(ref),
};

export function resolveCloudCredentialStore(): CredentialStore | undefined {
  return nativeAvailable() ? cloudCredentialStore : undefined;
}
