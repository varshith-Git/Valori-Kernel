"use client";

import Link from "next/link";
import { usePathname } from "next/navigation";
import { ChevronRight } from "lucide-react";

const LABELS: Record<string, string> = {
  "":           "Proof",
  search:       "Search",
  logs:         "Logs",
  metrics:      "Metrics",
  snapshots:    "Snapshots",
  audit:        "Audit Trail",
  auditor:      "Auditor Portal",
  cluster:      "Cluster",
  projects:     "Projects",
  settings:     "Settings",
  help:         "Feature Guide",
};

export function Breadcrumb() {
  const path = usePathname();
  // Split and decode each segment
  const segments = path.split("/").map((s) => decodeURIComponent(s));
  // Build crumbs: each crumb has { label, href }
  const crumbs: { label: string; href: string }[] = [];
  let running = "";
  for (const seg of segments) {
    running = running ? `${running}/${seg}` : `/${seg}`;
    const label = LABELS[seg] ?? (seg || "Proof");
    crumbs.push({ label, href: running === "/" ? "/" : running });
  }
  // Deduplicate root
  const visible = crumbs.filter((c, i) => !(i === 0 && c.href === "/") || crumbs.length === 1);

  if (visible.length <= 1) {
    return (
      <span className="text-sm font-medium text-accent-foreground">{visible[0]?.label ?? "Proof"}</span>
    );
  }

  return (
    <nav aria-label="Breadcrumb" className="flex items-center gap-1 text-sm min-w-0">
      {visible.map((c, i) => {
        const isLast = i === visible.length - 1;
        return (
          <span key={c.href} className="flex items-center gap-1 min-w-0">
            {i > 0 && <ChevronRight size={13} className="shrink-0 text-zinc-700" />}
            {isLast ? (
              <span className="font-medium text-card-foreground truncate">{c.label}</span>
            ) : (
              <Link
                href={c.href}
                className="text-muted-foreground hover:text-accent-foreground transition-colors truncate"
              >
                {c.label}
              </Link>
            )}
          </span>
        );
      })}
    </nav>
  );
}
