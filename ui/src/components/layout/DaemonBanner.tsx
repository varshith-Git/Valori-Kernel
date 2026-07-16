"use client";

import { useEffect, useRef, useState } from "react";
import { WifiOff, RefreshCw } from "lucide-react";
import { nativeAvailable, startDaemon, getPreference } from "@/lib/native";

const POLL_MS = 10_000;
const FAIL_THRESHOLD = 2;

export function DaemonBanner() {
  const [lost, setLost] = useState(false);
  const [restarting, setRestarting] = useState(false);
  const failCount = useRef(0);

  useEffect(() => {
    // Only poll inside the Tauri desktop shell — in a plain browser tab the
    // daemon concept doesn't apply and /api/health going offline is normal.
    if (!nativeAvailable()) return;

    const check = async () => {
      try {
        const res = await fetch("/api/health", { signal: AbortSignal.timeout(4000) });
        if (res.ok) {
          failCount.current = 0;
          setLost(false);
        } else {
          throw new Error(`${res.status}`);
        }
      } catch {
        failCount.current += 1;
        if (failCount.current >= FAIL_THRESHOLD) setLost(true);
      }
    };

    const id = setInterval(check, POLL_MS);
    return () => clearInterval(id);
  }, []);

  const handleRestart = async () => {
    setRestarting(true);
    try {
      const workspaceDir = await getPreference<string>("workspaceDir");
      await startDaemon(workspaceDir);
      failCount.current = 0;
      setLost(false);
    } catch (e) {
      console.error("failed to restart daemon:", e);
    } finally {
      setRestarting(false);
    }
  };

  if (!lost) return null;

  return (
    <div className="flex items-center justify-between gap-3 border-b border-destructive/30 bg-destructive/8 px-5 py-2.5 text-xs shrink-0">
      <div className="flex items-center gap-2 text-destructive">
        <WifiOff size={13} className="shrink-0" />
        <span className="font-medium">Connection to Valori daemon lost</span>
        <span className="text-destructive/70 hidden sm:inline">— API calls will fail until reconnected</span>
      </div>
      <button
        onClick={handleRestart}
        disabled={restarting}
        className="flex items-center gap-1.5 rounded-md border border-destructive/40 bg-destructive/10 px-3 py-1 font-medium text-destructive hover:bg-destructive/20 transition-colors disabled:opacity-60 shrink-0"
      >
        <RefreshCw size={11} className={restarting ? "animate-spin" : ""} />
        {restarting ? "Restarting…" : "Restart"}
      </button>
    </div>
  );
}
