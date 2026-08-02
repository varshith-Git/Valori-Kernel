"use client";

import { createContext, useContext, useEffect, useState } from "react";
import type { ReactNode } from "react";

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
    const stored = (localStorage.getItem("valori-theme") as ThemePref | null) ?? "dark";
    const resolved = resolveTheme(stored);
    setPref(stored);
    setResolvedTheme(resolved);
    applyTheme(resolved);

    // Keep system theme in sync with OS preference changes
    const mql = window.matchMedia("(prefers-color-scheme: dark)");
    const onMql = () => {
      const current = (localStorage.getItem("valori-theme") as ThemePref | null) ?? "dark";
      if (current === "system") {
        const r = resolveTheme("system");
        setResolvedTheme(r);
        applyTheme(r);
      }
    };
    mql.addEventListener("change", onMql);
    return () => mql.removeEventListener("change", onMql);
  }, []);

  const setTheme = (p: ThemePref) => {
    localStorage.setItem("valori-theme", p);
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
