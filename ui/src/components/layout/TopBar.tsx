"use client";

import { useEffect, useState } from "react";
import { usePathname, useRouter } from "next/navigation";
import { ChevronLeft, Download, X } from "lucide-react";
import { Breadcrumb } from "./Breadcrumb";
import { installUpdate, nativeAvailable } from "@/lib/native";

interface UpdateInfo {
  version: string;
  body: string;
}

function UpdateBanner({ info, onDismiss }: { info: UpdateInfo; onDismiss: () => void }) {
  const [installing, setInstalling] = useState(false);

  const handleInstall = async () => {
    setInstalling(true);
    try {
      await installUpdate();
      // app restarts — this line won't be reached
    } catch (e) {
      console.error("update failed:", e);
      setInstalling(false);
    }
  };

  return (
    <div className="shrink-0 flex items-center gap-2.5 px-4 py-1.5 bg-[var(--v-accent-muted)] border-b border-[var(--v-accent)]/30 text-xs">
      <Download size={12} className="text-[var(--v-accent)] shrink-0" />
      <span className="text-foreground font-medium">
        Valori {info.version} is available
      </span>
      {info.body && (
        <span className="text-muted-foreground hidden sm:inline truncate max-w-[260px]">
          — {info.body}
        </span>
      )}
      <div className="ml-auto flex items-center gap-2 shrink-0">
        <button
          onClick={handleInstall}
          disabled={installing}
          className="px-2.5 py-0.5 rounded bg-[var(--v-accent)] text-white font-semibold hover:bg-[var(--v-accent-ring)] disabled:opacity-60 transition-colors"
        >
          {installing ? "Installing…" : "Install & Restart"}
        </button>
        <button
          onClick={onDismiss}
          aria-label="Dismiss update"
          className="text-muted-foreground hover:text-foreground transition-colors"
        >
          <X size={13} />
        </button>
      </div>
    </div>
  );
}

export function TopBar() {
  const path = usePathname();
  const router = useRouter();
  const isRoot = path === "/";
  const [updateInfo, setUpdateInfo] = useState<UpdateInfo | null>(null);

  // Listen for the `update-available` event emitted by the Rust updater check.
  // Only runs inside the Tauri shell; no-ops in browser dev mode.
  useEffect(() => {
    if (!nativeAvailable()) return;
    let unlisten: (() => void) | undefined;
    import("@tauri-apps/api/event").then(({ listen }) => {
      listen<UpdateInfo>("update-available", (event) => {
        setUpdateInfo(event.payload);
      }).then((fn) => {
        unlisten = fn;
      });
    });
    return () => unlisten?.();
  }, []);

  // When the window uses titleBarStyle:"overlay", macOS native traffic lights
  // are 72 px wide and sit inside the window frame. We reserve that space so
  // the back button doesn't clash with them.
  const trafficLightPad = nativeAvailable() ? "pl-[76px]" : "pl-5";

  return (
    <>
      {updateInfo && (
        <UpdateBanner info={updateInfo} onDismiss={() => setUpdateInfo(null)} />
      )}
      <div
        data-tauri-drag-region
        className={`h-11 shrink-0 border-b border-border/60 bg-background/80 backdrop-blur-sm flex items-center gap-1.5 pr-5 ${trafficLightPad}`}
      >
        <button
          onClick={() => router.back()}
          aria-label="Go back"
          className={`h-6 w-6 flex items-center justify-center rounded-md text-muted-foreground hover:text-foreground hover:bg-accent transition-all duration-150 ${
            isRoot ? "opacity-0 pointer-events-none" : "opacity-100"
          }`}
        >
          <ChevronLeft size={13} />
        </button>
        <Breadcrumb />
      </div>
    </>
  );
}
