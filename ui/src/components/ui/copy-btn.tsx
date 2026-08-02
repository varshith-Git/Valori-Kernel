"use client";

import { useCallback, useState } from "react";

interface CopyBtnProps {
  text: string;
  label?: string;
  className?: string;
}

/**
 * Tiny clipboard copy button.
 * Previously defined independently in AskTab, CertifyTab, and snapshots/page.
 */
export function CopyBtn({ text, label = "copy", className = "" }: CopyBtnProps) {
  const [done, setDone] = useState(false);

  const copy = useCallback(async (e: React.MouseEvent) => {
    e.stopPropagation();
    try {
      if (navigator.clipboard) {
        await navigator.clipboard.writeText(text);
        setDone(true);
      } else {
        // Fallback for non-secure HTTP environments
        const textArea = document.createElement("textarea");
        textArea.value = text;
        textArea.style.position = "fixed";
        document.body.appendChild(textArea);
        textArea.focus();
        textArea.select();
        try {
          document.execCommand("copy");
          setDone(true);
        } catch (err) {
          console.error("Fallback copy failed", err);
        }
        document.body.removeChild(textArea);
      }
      setTimeout(() => setDone(false), 1500);
    } catch (err) {
      console.error("Clipboard copy failed", err);
    }
  }, [text]);

  return (
    <button
      onClick={copy}
      className={`text-xs px-2 py-1 rounded border transition-all shrink-0 ${
        done
          ? "border-emerald-500 text-emerald-600 dark:text-emerald-400"
          : "border-input text-muted-foreground hover:text-accent-foreground hover:border-ring"
      } ${className}`}
    >
      {done ? "✓" : label}
    </button>
  );
}
