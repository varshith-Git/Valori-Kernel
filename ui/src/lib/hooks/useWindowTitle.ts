"use client";

import { useEffect } from "react";
import { nativeAvailable } from "@/lib/native";

/**
 * Updates the native window title when inside the Tauri shell.
 * Falls back silently in the browser (no-op).
 */
export function useWindowTitle(title: string) {
  useEffect(() => {
    if (!nativeAvailable()) return;
    // Dynamic import so the Next.js bundle doesn't pull in @tauri-apps/api
    // when running in a plain browser (dev mode / web).
    import("@tauri-apps/api/window")
      .then(({ getCurrentWindow }) => getCurrentWindow().setTitle(title))
      .catch(() => {});
  }, [title]);
}
