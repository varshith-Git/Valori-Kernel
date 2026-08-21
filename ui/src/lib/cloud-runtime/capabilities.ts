import type { StudioCapabilities } from "@valori/studio";
import { nativeAvailable } from "@/lib/native";

// Actual Desktop Cloud behavior, not a copy of Local's:
//  - localFilesystem: false — a Cloud project has no local WAL/snapshot
//    files on this machine; there is nothing for Metrics'/Snapshots'
//    localFilesystem-gated cards to read.
//  - multiCollectionPicker: true — Cloud projects genuinely support
//    multiple named collections (CollectionsPanel, POST .../namespaces),
//    same product capability as Local. (The pre-Shared-Studio Playground
//    inferred this from `!projectId`, which happened to default Cloud to
//    false — an accident of the old code, not a real product constraint;
//    this makes the actual capability explicit instead.)
//  - osKeychain: real Tauri native-bridge availability, same detection
//    LocalRuntime uses — Desktop Cloud is still the Desktop/Tauri app, so
//    this can genuinely be true when running inside Tauri and false when
//    running in a browser-only dev environment.
//  - clientEmbeddingFallback: false — Desktop Cloud's ingest route has no
//    client-driven chunk+embed+insert fallback (Cloud projects require a
//    server embed provider); explicit rather than relying on the field's
//    optional/unset default, per Phase G2.
export function resolveCloudCapabilities(): StudioCapabilities {
  return {
    localFilesystem: false,
    multiCollectionPicker: true,
    osKeychain: nativeAvailable(),
    clientEmbeddingFallback: false,
  };
}
