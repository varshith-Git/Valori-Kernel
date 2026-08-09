"use client";

import { createContext, useContext, useEffect, useState } from "react";
import type { ReactNode } from "react";
import { getPreference, nativeAvailable, setPreference } from "@/lib/native";

export type ThemePref = "dark" | "light" | "system";
type Theme = "dark" | "light";

interface ThemeContextValue {
  theme: Theme;       // resolved (what's actually applied)
  pref: ThemePref;    // stored preference (may be "system")
  setTheme: (p: ThemePref) => void;
  toggle: () => void;
}

const ThemeContext = createContext<ThemeContextValue>({
  theme: "dark",
  pref: "dark",
  setTheme: () => {},
  toggle: () => {},
});

export function useTheme() {
  return useContext(ThemeContext);
}

// Legacy localStorage key, from before `studio.redb` existed (and briefly
// after, until this fix — see docs/architecture/studio-storage.md
// "Theme persistence"). Only ever read now — see loadTheme()'s one-time
// migration below and setTheme()'s desktop branch, neither of which write
// it anymore in the desktop app. Still the actual persistence mechanism
// for the browser/web build (Valori Cloud, or `ui/` run standalone via
// `npm run dev` outside Tauri), which has no `studio.redb` at all.
const LEGACY_LOCALSTORAGE_KEY = "valori-theme";

function resolveTheme(pref: ThemePref): Theme {
  if (pref === "system") {
    return window.matchMedia("(prefers-color-scheme: dark)").matches ? "dark" : "light";
  }
  return pref;
}

function applyTheme(theme: Theme) {
  const root = document.documentElement;
  root.classList.remove("dark", "light");
  root.classList.add(theme);
  root.setAttribute("data-theme", theme);
}

export function ThemeProvider({ children }: { children: ReactNode }) {
  const [pref, setPref] = useState<ThemePref>("dark");
  const [theme, setResolvedTheme] = useState<Theme>("dark");

  useEffect(() => {
    async function loadTheme() {
      let stored: ThemePref | null = null;

      if (nativeAvailable()) {
        // Desktop: studio.redb (via StudioPreferences) is the sole
        // authoritative store — see setTheme() below for the matching
        // write side.
        try {
          stored = await getPreference<ThemePref>("theme");
        } catch {
          stored = null;
        }

        if (!stored) {
          // One-time, idempotent migration for installations that set a
          // theme before `studio.redb` existed (or before preferences
          // were actually wired through it) and have never toggled theme
          // since — the only way `getPreference` above could still be
          // empty while a legacy value exists. Never deletes the legacy
          // key (same "read-only, never destructive" discipline as the
          // S2a migration engine); simply backfills studio.redb once.
          // After this, `getPreference("theme")` always finds a value, so
          // this branch never runs again for this installation — no
          // separate "have I migrated" flag is needed, the absence of a
          // stored value *is* the trigger condition, and it can only be
          // true once.
          const legacy =
            typeof localStorage !== "undefined"
              ? (localStorage.getItem(LEGACY_LOCALSTORAGE_KEY) as ThemePref | null)
              : null;
          if (legacy) {
            stored = legacy;
            setPreference("theme", legacy).catch(() => {});
          }
        }
      } else if (typeof localStorage !== "undefined") {
        // Browser/web mode: no studio.redb exists here at all —
        // localStorage remains the actual, unchanged persistence
        // mechanism (Valori Cloud's web UI, or `ui/` run standalone).
        stored = localStorage.getItem(LEGACY_LOCALSTORAGE_KEY) as ThemePref | null;
      }

      const initial = stored ?? "dark";
      const resolved = resolveTheme(initial);
      setPref(initial);
      setResolvedTheme(resolved);
      applyTheme(resolved);
    }

    loadTheme();

    // Keep system theme in sync with OS preference changes
    const mql = window.matchMedia("(prefers-color-scheme: dark)");
    const onMql = () => {
      setPref((current) => {
        if (current === "system") {
          const r = resolveTheme("system");
          setResolvedTheme(r);
          applyTheme(r);
        }
        return current;
      });
    };
    mql.addEventListener("change", onMql);
    return () => mql.removeEventListener("change", onMql);
  }, []);

  const setTheme = (p: ThemePref) => {
    if (nativeAvailable()) {
      // Desktop: studio.redb is the one authoritative store — no
      // localStorage dual-write (this used to write both; see
      // docs/architecture/studio-storage.md "Theme persistence" for why
      // that changed).
      setPreference("theme", p).catch(() => {});
    } else if (typeof localStorage !== "undefined") {
      // Browser/web mode: localStorage remains the persistence mechanism,
      // unchanged.
      localStorage.setItem(LEGACY_LOCALSTORAGE_KEY, p);
    }
    const resolved = resolveTheme(p);
    setPref(p);
    setResolvedTheme(resolved);
    applyTheme(resolved);
  };

  const toggle = () => {
    setTheme(theme === "dark" ? "light" : "dark");
  };

  return (
    <ThemeContext.Provider value={{ theme, pref, setTheme, toggle }}>
      {children}
    </ThemeContext.Provider>
  );
}
