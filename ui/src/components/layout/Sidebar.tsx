"use client";

import Link from "next/link";
import { usePathname, useRouter } from "next/navigation";
import { useState } from "react";
import { cn } from "@/lib/utils";
import { useCluster } from "@/lib/hooks/useCluster";
import { useProjectGroups } from "@/lib/hooks/useCollections";
import { useProjectManifest } from "@/lib/hooks/useProjectManifest";
import { useHealth } from "@/lib/hooks/useHealth";
import { CreateProjectDialog } from "@/components/projects/CreateProjectDialog";
import { ThemeToggle } from "@/components/layout/ThemeToggle";
import {
  ShieldCheck,
  Search,
  ScrollText,
  BarChart2,
  Archive,
  ClipboardList,
  UserCheck,
  Network,
  FolderOpen,
  ChevronRight,
  Plus,
  Settings,
  HelpCircle,
  Layers,
  Radio,
  Server,
  Rocket,
  Home,
} from "lucide-react";

/* --- Navigation sections -------------------------------------------- */

const MONITOR_NAV = [
  { href: "/proof",     label: "Proof",          Icon: ShieldCheck },
  { href: "/logs",      label: "Logs",           Icon: ScrollText  },
  { href: "/metrics",   label: "Metrics",        Icon: BarChart2   },
  { href: "/snapshots", label: "Snapshots",      Icon: Archive     },
];

const DATA_NAV = [
  { href: "/search",    label: "Search",         Icon: Search      },
];

const CONTROL_NAV = [
  { href: "/launch",    label: "Launcher",       Icon: Rocket      },
];

const COMPLIANCE_NAV = [
  { href: "/audit",     label: "Audit Trail",    Icon: ClipboardList },
  { href: "/auditor",   label: "Auditor Portal", Icon: UserCheck   },
];

/* --- Shared helpers ------------------------------------------------- */

type NavItem = {
  href: string;
  label: string;
  Icon: React.ComponentType<{ size?: number; className?: string }>;
};

function NavLink({ item, active }: { item: NavItem; active: boolean }) {
  return (
    <Link
      href={item.href}
      className={cn(
        "flex items-center gap-2.5 rounded-lg px-2.5 py-2 text-sm font-medium transition-all duration-150",
        active
          ? "bg-[var(--v-accent-muted)] text-foreground [box-shadow:inset_2px_0_0_var(--v-accent)]"
          : "text-muted-foreground hover:bg-accent/60 hover:text-foreground"
      )}
    >
      <item.Icon
        size={15}
        className={active ? "text-[var(--v-accent)]" : "text-muted-foreground"}
      />
      {item.label}
    </Link>
  );
}

function SectionHeader({ label }: { label: string }) {
  return (
    <p className="px-2.5 pt-4 pb-1 text-[9px] font-semibold uppercase tracking-[0.12em] text-muted-foreground select-none">
      {label}
    </p>
  );
}

/* --- Status footer (replaces the old top header bar) ---------------- */

function StatusFooter() {
  const { online, status } = useHealth();
  const { isStandalone, isLeader, members, nodeId } = useCluster();

  const dotColor =
    !online               ? "bg-red-400 animate-pulse" :
    status === "ok"       ? "bg-emerald-400"            :
    status === "degraded" ? "bg-amber-400"              :
                            "bg-red-400 animate-pulse";

  const textColor =
    !online               ? "text-red-400"     :
    status === "ok"       ? "text-emerald-400" :
    status === "degraded" ? "text-amber-400"   :
                            "text-red-400";

  const mode     = isStandalone ? "Standalone" : "Cluster";
  const ModeIcon = isStandalone ? Server : Radio;

  const statusLabel = !online
    ? "unreachable"
    : !isStandalone && members.length > 0
    ? isLeader ? "leader" : "follower"
    : status ?? "connected";

  return (
    <div className="border-t border-border/80 p-3 flex flex-col gap-2">
      {/* Mode + connection card */}
      <div className="rounded-lg bg-card border border-border px-3 py-2.5 flex items-center gap-2.5">
        <ModeIcon size={14} className="shrink-0 text-muted-foreground" />
        <div className="flex-1 min-w-0">
          <div className="flex items-center gap-1.5 flex-wrap">
            <span className="text-xs font-semibold text-card-foreground">{mode}</span>
            {!isStandalone && members.length > 0 && (
              <span className="text-[10px] text-muted-foreground tabular-nums">
                node-{nodeId} · {members.length} nodes
              </span>
            )}
          </div>
          <div className="flex items-center gap-1.5 mt-0.5">
            <span className={`h-1.5 w-1.5 rounded-full shrink-0 ${dotColor}`} />
            <span className={`text-[10px] ${textColor}`}>{statusLabel}</span>
          </div>
        </div>
      </div>

      {/* Settings + Help + theme toggle */}
      <div className="flex gap-1">
        <Link
          href="/settings"
          className="flex-1 flex items-center justify-center gap-1.5 rounded-md py-1.5 text-[11px] text-muted-foreground hover:text-accent-foreground hover:bg-accent/60 transition-colors"
        >
          <Settings size={11} />
          Settings
        </Link>
        <Link
          href="/help"
          className="flex-1 flex items-center justify-center gap-1.5 rounded-md py-1.5 text-[11px] text-muted-foreground hover:text-accent-foreground hover:bg-accent/60 transition-colors"
        >
          <HelpCircle size={11} />
          Help
        </Link>
        <ThemeToggle />
      </div>
    </div>
  );
}

/* --- Sidebar -------------------------------------------------------- */

export function Sidebar() {
  const path = usePathname();
  const router = useRouter();
  // All projects come from the on-disk manifest (works while nodes are stopped).
  // Collections under the *active* project come from the live, connected node.
  const { projects, isLoading, create, open } = useProjectManifest();
  const { groups } = useProjectGroups();
  const { isStandalone } = useCluster();
  const [createOpen, setCreateOpen] = useState(false);

  const isActive = (href: string) =>
    path === href || (href !== "/" && path.startsWith(href + "/"));

  return (
    <>
      <aside className="flex h-screen w-56 flex-col border-r border-border/80 bg-background flex-shrink-0">

        {/* Logo */}
        <div className="px-4 py-4 border-b border-border/80">
          <Link href="/" className="flex items-center gap-2">
            <div className="h-6 w-6 rounded-md bg-[var(--v-accent-muted)] flex items-center justify-center">
              <ShieldCheck size={13} className="text-[var(--v-accent)]" />
            </div>
            <div className="flex items-baseline gap-1">
              <span className="font-mono text-sm font-bold tracking-tight text-foreground">valori</span>
              <span className="font-mono text-[10px] text-muted-foreground">kernel</span>
            </div>
          </Link>
        </div>

        {/* Scrollable nav area */}
        <div className="flex-1 overflow-y-auto px-2 pb-2">

          {/* Home */}
          <nav className="flex flex-col gap-0.5 pt-2">
            <NavLink item={{ href: "/", label: "Home", Icon: Home }} active={path === "/"} />
          </nav>

          {/* Monitor */}
          <SectionHeader label="Monitor" />
          <nav className="flex flex-col gap-0.5">
            {MONITOR_NAV.map((n) => (
              <NavLink key={n.href} item={n} active={isActive(n.href)} />
            ))}
          </nav>

          {/* Data */}
          <SectionHeader label="Data" />
          <nav className="flex flex-col gap-0.5">
            {DATA_NAV.map((n) => (
              <NavLink key={n.href} item={n} active={isActive(n.href)} />
            ))}
            {!isStandalone && (
              <NavLink
                item={{ href: "/cluster", label: "Cluster", Icon: Network }}
                active={isActive("/cluster")}
              />
            )}
          </nav>

          {/* Compliance */}
          <SectionHeader label="Compliance" />
          <nav className="flex flex-col gap-0.5">
            {COMPLIANCE_NAV.map((n) => (
              <NavLink key={n.href} item={n} active={isActive(n.href)} />
            ))}
          </nav>

          {/* Control */}
          <SectionHeader label="Control" />
          <nav className="flex flex-col gap-0.5">
            {CONTROL_NAV.map((n) => (
              <NavLink key={n.href} item={n} active={isActive(n.href)} />
            ))}
          </nav>

          {/* Divider */}
          <div className="mx-1 my-3 border-t border-border/80" />

          {/* Projects section header */}
          <div className="flex items-center justify-between px-2.5 mb-1.5">
            <p className="text-[9px] font-semibold uppercase tracking-[0.12em] text-muted-foreground select-none">
              Projects
            </p>
            <button
              onClick={() => setCreateOpen(true)}
              title="New project"
              className="flex items-center gap-0.5 rounded-md px-1.5 py-0.5 text-[10px] font-medium text-muted-foreground hover:bg-accent hover:text-card-foreground transition-colors"
            >
              <Plus size={10} />
              New
            </button>
          </div>

          {/* Project list */}
          <div className="flex flex-col gap-0.5">
            {isLoading ? (
              <div className="flex flex-col gap-1.5 px-2 pt-1">
                {[1, 2, 3].map((i) => (
                  <div key={i} className="h-7 animate-pulse rounded-lg bg-accent/60" />
                ))}
              </div>
            ) : projects.length === 0 ? (
              <button
                onClick={() => setCreateOpen(true)}
                className="mx-1 mt-1 flex items-center justify-center gap-1.5 rounded-lg border border-dashed border-border py-3 text-xs text-muted-foreground hover:border-muted hover:text-muted-foreground transition-colors"
              >
                <Plus size={12} />
                Create first project
              </button>
            ) : (
              projects.map((p) => {
                const href = `/projects/${encodeURIComponent(p.name)}`;
                const active = path === href || path.startsWith(href + "/");
                const running = p.status === "running" || p.status === "starting";
                // Live collections only available for the active (connected) project.
                const cols = active ? (groups.find((g) => g.project === p.name)?.collections ?? []) : [];
                return (
                  <div key={p.name}>
                    <Link
                      href={href}
                      className={cn(
                        "group flex items-center justify-between rounded-lg px-2.5 py-1.5 text-xs transition-all duration-150",
                        active
                          ? "bg-[var(--v-accent-muted)] text-foreground [box-shadow:inset_2px_0_0_var(--v-accent)]"
                          : "text-muted-foreground hover:bg-accent/60 hover:text-card-foreground"
                      )}
                    >
                      <span className="flex items-center gap-1.5 truncate">
                        <Layers
                          size={11}
                          className={
                            active
                              ? "text-[var(--v-accent)]"
                              : "text-muted-foreground group-hover:text-muted-foreground"
                          }
                        />
                        <span className="truncate">{p.name}</span>
                      </span>
                      <span
                        className={cn(
                          "ml-1 h-1.5 w-1.5 rounded-full shrink-0",
                          running ? "bg-emerald-400" : "bg-muted-foreground/40"
                        )}
                        title={running ? "running" : "at rest"}
                      />
                    </Link>

                    {/* Inline collections under active project */}
                    {active && cols.length > 0 && (
                      <div className="ml-4 mt-0.5 flex flex-col gap-0.5 border-l border-border pl-2">
                        {cols.map((col) => {
                          const colHref = `${href}/${encodeURIComponent(col)}`;
                          return (
                            <Link
                              key={col}
                              href={colHref}
                              className={cn(
                                "flex items-center gap-1.5 rounded-md px-2 py-1.5 text-[11px] transition-all duration-150 truncate",
                                path === colHref
                                  ? "bg-accent text-foreground"
                                  : "text-muted-foreground hover:bg-accent/60 hover:text-accent-foreground"
                              )}
                            >
                              <ChevronRight size={10} className="shrink-0 opacity-40" />
                              <span className="truncate">{col}</span>
                            </Link>
                          );
                        })}
                      </div>
                    )}
                  </div>
                );
              })
            )}
          </div>

        </div>

        {/* Connection + mode status at the very bottom */}
        <StatusFooter />
      </aside>

      <CreateProjectDialog
        open={createOpen}
        onClose={() => setCreateOpen(false)}
        onCreate={async (name, dim, index) => {
          const entry = await create({ name, dim, index });
          if (!entry) return;
          await open(name);
          router.push(`/projects/${encodeURIComponent(name)}`);
        }}
      />
    </>
  );
}
