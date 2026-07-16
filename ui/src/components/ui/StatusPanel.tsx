import { cva, type VariantProps } from "class-variance-authority";
import { cn } from "@/lib/utils";

/**
 * Larger status banner (icon + title + message), for a single prominent
 * state like "hash match" / "tamper detected" / "no baseline yet". Same
 * light/dark-safe tone palette as StatusBadge, just bigger — one canonical
 * banner instead of every tab hand-rolling its own emerald-950/red-950 box.
 */
const panelVariants = cva("rounded-xl border px-5 py-5 flex items-start gap-4", {
  variants: {
    tone: {
      success: "border-emerald-500/30 bg-emerald-500/10",
      warning: "border-amber-500/30 bg-amber-500/10",
      error: "border-red-500/30 bg-red-500/10",
      neutral: "border-input bg-accent/50",
    },
  },
  defaultVariants: { tone: "neutral" },
});

const iconVariants = cva("text-3xl flex-shrink-0", {
  variants: {
    tone: {
      success: "text-emerald-600 dark:text-emerald-400",
      warning: "text-amber-600 dark:text-amber-400",
      error: "text-red-600 dark:text-red-400",
      neutral: "text-muted-foreground text-2xl",
    },
  },
  defaultVariants: { tone: "neutral" },
});

const titleVariants = cva("text-base font-bold tracking-wide", {
  variants: {
    tone: {
      success: "text-emerald-600 dark:text-emerald-400",
      warning: "text-amber-600 dark:text-amber-400",
      error: "text-red-600 dark:text-red-400",
      neutral: "text-sm font-medium text-muted-foreground",
    },
  },
  defaultVariants: { tone: "neutral" },
});

const messageVariants = cva("text-xs mt-1", {
  variants: {
    tone: {
      success: "text-emerald-700 dark:text-emerald-500",
      warning: "text-amber-700 dark:text-amber-500",
      error: "text-red-700 dark:text-red-500",
      neutral: "text-muted-foreground",
    },
  },
  defaultVariants: { tone: "neutral" },
});

export interface StatusPanelProps extends VariantProps<typeof panelVariants> {
  icon: React.ReactNode;
  title: string;
  message?: React.ReactNode;
  children?: React.ReactNode;
  className?: string;
}

export function StatusPanel({ tone, icon, title, message, children, className }: StatusPanelProps) {
  return (
    <div className={cn(panelVariants({ tone }), className)}>
      <span className={iconVariants({ tone })}>{icon}</span>
      <div className="min-w-0 flex-1">
        <p className={titleVariants({ tone })}>{title}</p>
        {message && <p className={messageVariants({ tone })}>{message}</p>}
        {children}
      </div>
    </div>
  );
}
