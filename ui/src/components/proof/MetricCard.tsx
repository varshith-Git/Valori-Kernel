interface Props {
  label: string;
  value: string | number | null;
  sub?: string;
}

export function MetricCard({ label, value, sub }: Props) {
  return (
    <div className="rounded-lg border border-border bg-card px-5 py-4">
      <p className="text-xs text-muted-foreground uppercase tracking-widest">{label}</p>
      <p className="mt-1.5 font-mono text-2xl font-semibold text-foreground">
        {value ?? <span className="text-muted-foreground">—</span>}
      </p>
      {sub && <p className="mt-0.5 text-xs text-muted-foreground">{sub}</p>}
    </div>
  );
}
