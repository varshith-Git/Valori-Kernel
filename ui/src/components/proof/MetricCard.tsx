interface Props {
  label: string;
  value: string | number | null;
  sub?: string;
}

export function MetricCard({ label, value, sub }: Props) {
  return (
    <div className="rounded-lg border border-zinc-800 bg-zinc-900 px-5 py-4">
      <p className="text-xs text-zinc-500 uppercase tracking-widest">{label}</p>
      <p className="mt-1.5 font-mono text-2xl font-semibold text-white">
        {value ?? <span className="text-zinc-600">—</span>}
      </p>
      {sub && <p className="mt-0.5 text-xs text-zinc-600">{sub}</p>}
    </div>
  );
}
