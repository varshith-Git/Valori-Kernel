"use client";

import { Moon, Sun } from "lucide-react";
import { useTheme } from "@/lib/theme";
import { cn } from "@/lib/utils";

export function ThemeToggle({ className }: { className?: string }) {
  const { theme, toggle } = useTheme();
  const isDark = theme === "dark";

  return (
    <button
      onClick={toggle}
      title={isDark ? "Switch to light mode" : "Switch to dark mode"}
      aria-label={isDark ? "Switch to light mode" : "Switch to dark mode"}
      className={cn(
        "flex items-center justify-center rounded-md p-1.5 transition-all duration-200",
        "text-muted-foreground hover:text-card-foreground hover:bg-accent/70",
        "dark:text-muted-foreground dark:hover:text-card-foreground",
        "light:text-muted-foreground light:hover:text-zinc-700",
        className
      )}
    >
      {isDark ? (
        <Sun size={14} className="transition-transform duration-300 rotate-0 hover:rotate-12" />
      ) : (
        <Moon size={14} className="transition-transform duration-300" />
      )}
    </button>
  );
}
