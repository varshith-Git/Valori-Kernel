import type { StudioCapabilities } from "@valori/studio";
import { nativeAvailable } from "@/lib/native";

// Desktop Local's capability set. `localFilesystem`/`multiCollectionPicker`
// are unconditionally true — matches exactly what MetricsView/PlaygroundView
// already inferred from `!projectId` before Phase C (the browser and the
// locally-connected node always share a filesystem; the collection picker
// always has real namespaces to switch between via the shared daemon
// connection). `osKeychain` reflects whether a CredentialStore is actually
// backed by the OS keychain right now — `false` outside Tauri (e.g. `next
// dev` run standalone in a browser), where useEmbeddingConfig/useLLMConfig
// correctly fall back to plain localStorage instead.
export function resolveLocalCapabilities(): StudioCapabilities {
  return {
    localFilesystem: true,
    multiCollectionPicker: true,
    osKeychain: nativeAvailable(),
    // Local's own /api/ingest route can chunk+embed+insert itself when the
    // node has no server embed provider configured (the "client pipeline"
    // fallback) — see studio's DocumentUploadTab module comment. Neither
    // Cloud host has an equivalent ingest-route fallback, so this stays
    // Local-only, not inferred from `!projectId` or any other heuristic.
    clientEmbeddingFallback: true,
  };
}
