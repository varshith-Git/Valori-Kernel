/**
 * The per-project feature set every host already exposes today, under
 * three different route prefixes (/projects/[name]/*, /cloud/projects/[id]/*,
 * /dashboard/projects/[id]/*) — confirmed identical in substance by the
 * atomic investigation. This describes *features*, not routes: a host's own
 * Sidebar/AppSidebar builds its actual `<Link href>` from `key`, so adding a
 * new project feature here is the one edit every host's nav picks up,
 * instead of three independently hand-maintained nav lists.
 */
export interface ProjectNavItem {
  key: string;
  label: string;
}

export const PROJECT_FEATURE_NAV: ProjectNavItem[] = [
  { key: "overview", label: "Overview" },
  { key: "metrics", label: "Metrics" },
  { key: "cluster", label: "Cluster" },
  { key: "graph", label: "Graph" },
  { key: "operations", label: "Operations" },
  { key: "tools", label: "Tools" },
  { key: "proof", label: "Proof" },
  { key: "snapshots", label: "Snapshots" },
  { key: "playground", label: "Playground" },
];
