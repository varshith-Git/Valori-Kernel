"use client";

import { useEffect, useState } from "react";

/**
 * Resolved "dark" | "light", read from the host's own `<html>` element
 * (every host already stamps `class="dark"`/`"light"` there for its own
 * Tailwind theming — see globals.css in both Valori-Kernel/ui and
 * valori-ui/ui) rather than from a host-specific theme provider/hook.
 * This is the one piece CodePanel.tsx needs (syntax-highlight token
 * colors) — deliberately not the full theme *preference* system (system/
 * dark/light, persistence), which stays entirely the host's concern.
 */
export function useResolvedTheme(): "dark" | "light" {
  const [theme, setTheme] = useState<"dark" | "light">(() =>
    typeof document !== "undefined" && document.documentElement.classList.contains("light")
      ? "light"
      : "dark"
  );

  useEffect(() => {
    const root = document.documentElement;
    const read = () => setTheme(root.classList.contains("light") ? "light" : "dark");
    read();
    const observer = new MutationObserver(read);
    observer.observe(root, { attributes: true, attributeFilter: ["class"] });
    return () => observer.disconnect();
  }, []);

  return theme;
}
