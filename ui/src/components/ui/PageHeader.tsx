/**
 * The one page-header shape every screen should share: title + subtitle on
 * the left, actions on the right. Before this, every page hand-rolled its
 * own `<h1 className="text-2xl font-semibold ...">` with a different size/
 * weight each time (`text-xl`, `text-lg`, `text-2xl` all appeared for what
 * was semantically the same "page title" role) — this is the fix for that,
 * not another one-off.
 */
export function PageHeader({
  title,
  subtitle,
  actions,
}: {
  title: React.ReactNode;
  subtitle?: React.ReactNode;
  actions?: React.ReactNode;
}) {
  return (
    <div className="mb-6 flex items-start justify-between gap-4">
      <div>
        <h1 className="text-2xl font-semibold tracking-tight text-foreground">{title}</h1>
        {subtitle && <p className="mt-1.5 text-sm text-muted-foreground">{subtitle}</p>}
      </div>
      {actions && <div className="flex shrink-0 items-center gap-2">{actions}</div>}
    </div>
  );
}
