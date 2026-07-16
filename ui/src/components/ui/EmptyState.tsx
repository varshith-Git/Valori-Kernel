import type { LucideIcon } from "lucide-react";
import { cn } from "@/lib/utils";

/**
 * One canonical "there's nothing here (yet / anymore / at all)" state —
 * icon + title + short explanation + optional action. Every page had been
 * hand-rolling its own version of this (compare the old ExecutionExplorer
 * empty state, Dashboard's "no projects", etc.) with slightly different
 * spacing and copy tone each time.
 */
export function EmptyState({
  icon: Icon,
  title,
  description,
  action,
  className,
}: {
  icon?: LucideIcon;
  title: string;
  description?: string;
  action?: React.ReactNode;
  className?: string;
}) {
  return (
    <div
      className={cn(
        "flex flex-col items-center justify-center gap-2 rounded-xl border border-dashed border-border/60 bg-muted/20 px-6 py-12 text-center",
        className
      )}
    >
      {Icon && <Icon className="h-6 w-6 text-muted-foreground" strokeWidth={1.5} />}
      <p className="text-sm font-medium text-foreground">{title}</p>
      {description && <p className="max-w-sm text-xs text-muted-foreground">{description}</p>}
      {action && <div className="mt-2">{action}</div>}
    </div>
  );
}
