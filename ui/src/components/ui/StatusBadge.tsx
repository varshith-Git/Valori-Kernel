import { cva, type VariantProps } from "class-variance-authority";
import { cn } from "@/lib/utils";

/**
 * Semantic status pill (success/warning/error/info/neutral), each with a
 * matching dot. Replaces the hand-rolled `<span className="rounded-full
 * border ...">` pattern that had drifted into a slightly different shape on
 * every page (project status, connection state, receipt verified, etc.) —
 * one canonical status indicator, used everywhere a status is shown.
 */
const dotVariants = cva("inline-block h-1.5 w-1.5 shrink-0 rounded-full", {
  variants: {
    tone: {
      success: "bg-emerald-500",
      warning: "bg-amber-500",
      error: "bg-red-500",
      info: "bg-[var(--v-accent)]",
      neutral: "bg-muted-foreground",
    },
  },
  defaultVariants: { tone: "neutral" },
});

const pillVariants = cva(
  "inline-flex items-center gap-1.5 rounded-full border px-2.5 py-0.5 text-xs font-medium whitespace-nowrap",
  {
    variants: {
      tone: {
        success: "border-emerald-500/20 bg-emerald-500/10 text-emerald-600 dark:text-emerald-400",
        warning: "border-amber-500/20 bg-amber-500/10 text-amber-600 dark:text-amber-400",
        error: "border-red-500/20 bg-red-500/10 text-red-600 dark:text-red-400",
        info: "border-[var(--v-accent)]/20 bg-[var(--v-accent-muted)] text-[var(--v-accent)]",
        neutral: "border-border bg-muted/60 text-muted-foreground",
      },
    },
    defaultVariants: { tone: "neutral" },
  }
);

export interface StatusBadgeProps extends VariantProps<typeof pillVariants> {
  children: React.ReactNode;
  /** Pulse the dot — for a transient state like "starting" or "connecting". */
  pulse?: boolean;
  className?: string;
}

export function StatusBadge({ tone, pulse, children, className }: StatusBadgeProps) {
  return (
    <span className={cn(pillVariants({ tone }), className)}>
      <span className={cn(dotVariants({ tone }), pulse && "animate-pulse")} />
      {children}
    </span>
  );
}
