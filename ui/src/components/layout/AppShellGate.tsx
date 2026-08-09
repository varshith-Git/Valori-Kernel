"use client";

import { useEffect, useRef, useState } from "react";
import { usePathname, useRouter } from "next/navigation";
import { Sidebar } from "@/components/layout/Sidebar";
import { TopBar } from "@/components/layout/TopBar";
import { Toaster } from "@/components/ui/toaster";
import { SettingsModal } from "@/components/settings/SettingsModal";
import Welcome from "@/components/onboarding/Welcome";
import { SignInGate } from "@/components/onboarding/SignInGate";
import { useWindowTitle } from "@/lib/hooks/useWindowTitle";
import {
  getLastPage,
  getPreference,
  getStudioRecoveryStatus,
  isOnboardingComplete,
  nativeAvailable,
  setLastPage,
  startDaemon,
} from "@/lib/native";
import { toast } from "@/lib/toast";
import { reportSessionEnded, reportSessionStarted } from "@/lib/telemetry";
import { markStartupPhase } from "@/lib/startupMarks";
import { createClient } from "@/utils/supabase/client";

// Paths that render full-screen without the sidebar/topbar shell.
// These must never be saved as "last page" or shown inside the app chrome.
const SHELL_EXEMPT = ["/login", "/signup", "/forgot-password", "/auth/"];

// Module-load time as a proxy for "app start" — not a true process-launch
// timestamp (that would need a Rust-side mark threaded through), but this
// module loads early enough in the app's lifecycle to be a reasonable
// approximation for Phase 1's startup_time_ms.
const moduleLoadTime = typeof performance !== "undefined" ? performance.now() : 0;
// react_mounted_ms for the startup waterfall (see startupMarks.ts) — same
// moment as moduleLoadTime above, just recorded as a wall-clock mark
// instead of a performance.now() delta.
markStartupPhase("react_mounted_ms");

function isExempt(path: string) {
  return SHELL_EXEMPT.some((p) => path.startsWith(p));
}

/** Gates the normal app shell behind a first-run Welcome flow — but only
 *  inside the desktop shell (`nativeAvailable()`). Folder pickers and a
 *  "which machine is this" installation step don't make sense for a plain
 *  browser tab hitting a hosted `ui/` deployment, so that path renders the
 *  app immediately, exactly as it always has.
 *
 *  Also restores the last-visited page on launch and keeps it updated as you
 *  navigate — small "app memory" polish, desktop only. */
export function AppShellGate({ children }: { children: React.ReactNode }) {
  const [ready, setReady] = useState(false);
  const [showWelcome, setShowWelcome] = useState(false);
  const [showSignIn, setShowSignIn] = useState(false);
  const [settingsOpen, setSettingsOpen] = useState(false);
  const pathname = usePathname();
  const router = useRouter();
  const restoredRef = useRef(false);

  useEffect(() => {
    if (!process.env.NEXT_PUBLIC_SUPABASE_URL) return;
    const supabase = createClient();

    // Check current session state immediately
    supabase.auth.getSession().then(({ data: { session } }) => {
      if (session) {
        setShowSignIn(false);
      } else {
        setShowSignIn(true);
      }
    });

    const { data: { subscription } } = supabase.auth.onAuthStateChange((event, session) => {
      if (session) {
        setShowSignIn(false);
      } else {
        setShowSignIn(true);
      }
    });
    return () => {
      subscription.unsubscribe();
    };
  }, []);

  useEffect(() => {
    (async () => {
      try {
        if (nativeAvailable()) {
          const complete = await isOnboardingComplete();
          if (!complete) {
            setShowWelcome(true);
          } else {
            const workspaceDir = await getPreference<string>("workspaceDir");
            const modelDir = await getPreference<string>("modelDir");
            startDaemon(workspaceDir, modelDir)
              .then(() => markStartupPhase("daemon_ready_ms"))
              .catch((e) => console.error("failed to start daemon:", e));
            // Returning user, onboarding (and its consent step) already
            // done — a no-op if `analytics` consent is off. First-time
            // users get this from handleOnboardingFinish instead, once
            // consent is actually known.
            reportSessionStarted(performance.now() - moduleLoadTime).catch(() => {});
          }
        }
      } catch (e) {
        console.error("AppShellGate native init failed:", e);
      } finally {
        setReady(true);
      }
    })();
  }, []);

  // Studio database recovery notice — non-blocking, per
  // docs/architecture/studio-storage.md §"Recovery UI". Queried once per
  // mount rather than only listening for the `studio-recovery` event: the
  // event fires once, synchronously, during Rust `setup()`, well before
  // this component (or any window) is guaranteed to be listening. A
  // healthy launch (the overwhelming common case) is silent — nothing is
  // shown, no toast, no delay.
  useEffect(() => {
    if (!nativeAvailable()) return;
    getStudioRecoveryStatus()
      .then((status) => {
        if (!status || status.kind === "healthy") return;
        if (status.kind === "unavailable") {
          toast(
            "Studio's local settings database is unavailable for this session. " +
              "Your project data was not affected.",
            "warning",
          );
          return;
        }
        // "restored_from_backup" | "fresh_database_created" — both carry
        // a ready-made, non-technical message from the Rust side.
        toast(status.message, "warning");
      })
      .catch(() => {});
  }, []);

  const handleOnboardingFinish = async () => {
    setShowWelcome(false);
    // First-time session_started — consent is only known now that the
    // onboarding wizard's telemetry step has actually run.
    reportSessionStarted(performance.now() - moduleLoadTime).catch(() => {});
    // After installation, check session — show sign-in gate if not signed in.
    if (process.env.NEXT_PUBLIC_SUPABASE_URL) {
      const supabase = createClient();
      const { data } = await supabase.auth.getSession();
      if (!data.session) { setShowSignIn(true); return; }
    }
  };

  // Restore last page exactly once, right after the gate opens — only when
  // landing on the bare root (a real deep link should win over "remembered"
  // state, so this never fights a URL you navigated to on purpose).
  useEffect(() => {
    if (!ready || showWelcome || restoredRef.current || !nativeAvailable()) return;
    restoredRef.current = true;
    if (pathname === "/") {
      getLastPage().then((last) => {
        // Never restore to auth pages — they're transient, not destinations.
        if (last && last !== "/" && !isExempt(last)) router.replace(last);
      }).catch(() => {});
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [ready, showWelcome]);

  // Keep it updated as you navigate (skip auth pages — they're not worth
  // saving as a "last page" since they're a means to an end, not a place).
  useEffect(() => {
    if (!ready || showWelcome || !nativeAvailable()) return;
    if (!isExempt(pathname)) setLastPage(pathname).catch(() => {});
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

  // Listen for the settings modal event (fired from SettingsPopover "All settings")
  useEffect(() => {
    const h = () => setSettingsOpen(true);
    window.addEventListener("valori:open-settings", h);
    return () => window.removeEventListener("valori:open-settings", h);
  }, []);

  // Best-effort session_ended — see telemetry.ts's own comment on why this
  // isn't fully reliable (a force-quit can skip beforeunload entirely).
  // Accepted gap for Phase 1.
  useEffect(() => {
    const h = () => { reportSessionEnded(); };
    window.addEventListener("beforeunload", h);
    return () => window.removeEventListener("beforeunload", h);
  }, []);

  // Global keyboard shortcuts (desktop-grade feel).
  useEffect(() => { // eslint-disable-line react-hooks/rules-of-hooks
    function onKey(e: KeyboardEvent) {
      if (!e.metaKey && !e.ctrlKey) return;
      switch (e.key) {
        case ",":
          e.preventDefault();
          setSettingsOpen(true);
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

  // Auth pages (login, signup, forgot-password, auth callbacks) must fill the
  // whole window — no sidebar, no topbar, no chrome.
  if (isExempt(pathname)) {
    return <>{children}</>;
  }

  if (showWelcome) {
    return (
      <div
        style={{
          position: "fixed",
          inset: 0,
          display: "flex",
          alignItems: "center",
          justifyContent: "center",
          background: "var(--background)",
        }}
      >
        <div
          style={{
            width: 760,
            height: 520,
            maxWidth: "calc(100vw - 48px)",
            maxHeight: "calc(100vh - 48px)",
            borderRadius: 12,
            overflow: "hidden",
            border: "1px solid var(--border)",
            boxShadow: "0 24px 64px rgba(0,0,0,0.22), 0 4px 16px rgba(0,0,0,0.12)",
            display: "flex",
            flexDirection: "column",
          }}
        >
          <Welcome onFinish={handleOnboardingFinish} />
        </div>
      </div>
    );
  }

  if (showSignIn) {
    return <SignInGate onSignedIn={() => setShowSignIn(false)} />;
  }

  return (
    <>
      <Sidebar />
      <div className="flex flex-1 flex-col overflow-hidden">
        <TopBar />
        <main className="flex-1 overflow-auto px-7 py-7">{children}</main>
      </div>
      <Toaster />
      <SettingsModal open={settingsOpen} onClose={() => setSettingsOpen(false)} />
    </>
  );
}
