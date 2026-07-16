"use client";

import { useEffect, useRef, useState } from "react";
import { usePathname, useRouter } from "next/navigation";
import { Sidebar } from "@/components/layout/Sidebar";
import { TopBar } from "@/components/layout/TopBar";
import { Toaster } from "@/components/ui/Toaster";
import Welcome from "@/components/onboarding/Welcome";
import { useWindowTitle } from "@/lib/hooks/useWindowTitle";
import {
  getLastPage,
  getPreference,
  isOnboardingComplete,
  nativeAvailable,
  setLastPage,
  startDaemon,
} from "@/lib/native";

/** Gates the normal app shell behind a first-run Welcome flow — but only
 *  inside the desktop shell (`nativeAvailable()`). Folder pickers and a
 *  "which machine is this" installation step don't make sense for a plain
 *  browser tab hitting a hosted `ui/` deployment, so that path renders the
 *  app immediately, exactly as it always has.
 *
 *  Also restores the last-visited page on launch and keeps it updated as you
 *  navigate — small "app memory" polish, desktop only. */
export function AppShellGate({ children }: { children: React.ReactNode }) {
  // TEMP diagnostic (Phase D1.3 blank-window debugging) — fires on every
  // render, including the very first, before any effect runs. Remove once
  // resolved.
  if (typeof window !== "undefined") {
    fetch("/api/diag-mount?stage=appshellgate-render").catch(() => {});
  }

  const [ready, setReady] = useState(false);
  const [showWelcome, setShowWelcome] = useState(false);
  const pathname = usePathname();
  const router = useRouter();
  const restoredRef = useRef(false);

  useEffect(() => {
    // TEMP diagnostic (Phase D1.3 blank-window debugging) — confirms React
    // actually mounted and ran, regardless of what happens below. Remove
    // once resolved.
    fetch("/api/diag-mount?stage=appshellgate-effect-fired").catch(() => {});
    (async () => {
      try {
        if (nativeAvailable()) {
          const complete = await isOnboardingComplete();
          setShowWelcome(!complete);
          // Returning user: launch the daemon against the workspace they picked
          // during onboarding. (First-run users get this from Welcome's finish()
          // instead, once a workspace has actually been chosen.)
          if (complete) {
            const workspaceDir = await getPreference<string>("workspaceDir");
            startDaemon(workspaceDir).catch((e) => console.error("failed to start daemon:", e));
          }
        }
      } catch (e) {
        // A thrown error here used to leave `ready` false forever — the
        // component renders `null` until `ready` flips true, so any
        // unhandled failure in the native calls above meant a permanently
        // blank page. Always proceed to `finally` so the app shows up even
        // if a native call fails.
        console.error("AppShellGate native init failed:", e);
        fetch(`/api/diag-mount?stage=appshellgate-init-error&msg=${encodeURIComponent(String(e))}`).catch(() => {});
      } finally {
        setReady(true);
      }
    })();
  }, []);

  // Restore last page exactly once, right after the gate opens — only when
  // landing on the bare root (a real deep link should win over "remembered"
  // state, so this never fights a URL you navigated to on purpose).
  useEffect(() => {
    if (!ready || showWelcome || restoredRef.current || !nativeAvailable()) return;
    restoredRef.current = true;
    if (pathname === "/") {
      getLastPage().then((last) => {
        if (last && last !== "/") router.replace(last);
      }).catch(() => {});
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [ready, showWelcome]);

  // Keep it updated as you navigate.
  useEffect(() => {
    if (!ready || showWelcome || !nativeAvailable()) return;
    setLastPage(pathname).catch(() => {});
  }, [ready, showWelcome, pathname]);

  // Derive a human-readable title from the current path and update the
  // native window titlebar (no-op in browser).
  const pageTitle = (() => {
    if (pathname === "/") return "Valori — Workspace";
    const seg = pathname.split("/").filter(Boolean);
    if (seg[0] === "projects" && seg[1]) return `Valori — ${decodeURIComponent(seg[1])}`;
    const label = seg[0].charAt(0).toUpperCase() + seg[0].slice(1);
    return `Valori — ${label}`;
  })();
  useWindowTitle(pageTitle); // eslint-disable-line react-hooks/rules-of-hooks

  // Global keyboard shortcuts (desktop-grade feel).
  useEffect(() => { // eslint-disable-line react-hooks/rules-of-hooks
    function onKey(e: KeyboardEvent) {
      if (!e.metaKey && !e.ctrlKey) return;
      switch (e.key) {
        case ",":
          e.preventDefault();
          router.push("/settings");
          break;
        case "r":
          if (!e.shiftKey) { e.preventDefault(); window.location.reload(); }
          break;
        case "[":
          e.preventDefault();
          history.back();
          break;
        case "]":
          e.preventDefault();
          history.forward();
          break;
      }
    }
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [router]);

  if (!ready) return null;

  if (showWelcome) {
    return <Welcome onFinish={() => setShowWelcome(false)} />;
  }

  return (
    <>
      <Sidebar />
      <div className="flex flex-1 flex-col overflow-hidden">
        <TopBar />
        <main className="flex-1 overflow-auto px-7 py-7">{children}</main>
      </div>
      <Toaster />
    </>
  );
}
