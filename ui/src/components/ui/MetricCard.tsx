import { cn } from "@/lib/utils";

/**
 * "Label above a big number" — the exact div this app was re-typing
 * (`rounded-xl border ... p-4 text-center` + a `text-xs muted-foreground`
 * label + a `font-mono text-2xl font-bold` value) in at least three
 * different places (Dashboard overview, Execution Explorer stats, the
 * Metrics tab) with small, accidental differences each time.
 */
export function MetricCard({
  label,
  value,
  hint,
  className,
}: {
  label: string;
  value: React.ReactNode;
  hint?: string;
  className?: string;
}) {
  return (
    <div className={cn("rounded-xl border border-border/60 bg-background/60 p-4 text-center", className)}>
      <span className="block text-xs text-muted-foreground mb-1">{label}</span>
      <span className="font-mono text-2xl font-bold text-foreground">{value}</span>
      {hint && <span className="mt-1 block text-[11px] text-muted-foreground">{hint}</span>}
    </div>
  );
}
