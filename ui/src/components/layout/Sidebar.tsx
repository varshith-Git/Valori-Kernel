"use client";

import Link from "next/link";
import Image from "next/image";
import { usePathname, useRouter } from "next/navigation";
import { useState, useEffect, useRef } from "react";
import { cn } from "@/lib/utils";
import { useCluster } from "@/lib/hooks/useCluster";
import { useProjectManifest } from "@/lib/hooks/useProjectManifest";
import { useHealth } from "@/lib/hooks/useHealth";
import { CreateProjectDialog } from "@/components/projects/CreateProjectDialog";
import { getPreference, nativeAvailable } from "@/lib/native";
import { SettingsPopover, type PopoverPos } from "@/components/layout/SettingsPopover";
import {
  ShieldCheck,
  Network,
  ChevronRight,
  ChevronLeft,
  Plus,
  Settings,
  Layers,
  Radio,
  Server,
  Rocket,
  Home,
  ScrollText,
  Search,
  Activity,
  BarChart2,
  SquareTerminal,
} from "lucide-react";

/* --- Helpers -------------------------------------------------------- */

type NavItem = {
  href: string;
  label: string;
  Icon: React.ComponentType<{ size?: number; className?: string }>;
};

function NavLink({
  item,
  active,
  collapsed,
}: {
  item: NavItem;
  active: boolean;
  collapsed: boolean;
}) {
  return (
    <Link
      href={item.href}
      title={collapsed ? item.label : undefined}
      aria-label={collapsed ? item.label : undefined}
      className={cn(
        "flex items-center rounded-lg text-sm font-medium transition-all duration-150",
        collapsed
          ? "h-9 w-9 mx-auto justify-center"
          : "gap-2.5 px-2.5 py-2",
        active
          ? "bg-[var(--v-accent-muted)] text-foreground [box-shadow:inset_2px_0_0_var(--v-accent)]"
          : "text-muted-foreground hover:bg-accent/60 hover:text-foreground"
      )}
    >
      <item.Icon
        size={15}
        className={active ? "text-[var(--v-accent)]" : "text-muted-foreground"}
      />
      {!collapsed && item.label}
    </Link>
  );
}

/* --- Status footer -------------------------------------------------- */

function StatusFooter({
  collapsed,
  settingsOpen,
  onSettingsToggle,
}: {
  collapsed: boolean;
  settingsOpen: boolean;
  onSettingsToggle: () => void;
}) {
  const settingsBtnRef = useRef<HTMLButtonElement | null>(null);
  const [popoverPos, setPopoverPos] = useState<PopoverPos | null>(null);
  const { online, status } = useHealth();
  const { isStandalone, isLeader, members, nodeId } = useCluster();

  const dotColor =
    !online               ? "bg-red-400 animate-pulse" :
    status === "ok"       ? "bg-emerald-500 dark:bg-emerald-400" :
    status === "degraded" ? "bg-amber-500 dark:bg-amber-400" :
                            "bg-red-500 dark:bg-red-400 animate-pulse";

  const textColor =
    !online               ? "text-red-600 dark:text-red-400" :
    status === "ok"       ? "text-emerald-600 dark:text-emerald-400" :
    status === "degraded" ? "text-amber-600 dark:text-amber-400" :
                            "text-red-600 dark:text-red-400";

  const mode     = isStandalone ? "Standalone" : "Cluster";
  const ModeIcon = isStandalone ? Server : Radio;

  const statusLabel = !online
    ? "unreachable"
    : !isStandalone && members.length > 0
    ? isLeader ? "leader" : "follower"
    : status ?? "connected";

  function handleSettingsToggle() {
    if (!settingsOpen && settingsBtnRef.current) {
      const r = settingsBtnRef.current.getBoundingClientRect();
      setPopoverPos({ left: r.left, bottom: window.innerHeight - r.top + 6 });
    }
    onSettingsToggle();
  }

  return (
    <div className="border-t border-border/80 p-3 flex flex-col gap-2">
      <SettingsPopover open={settingsOpen} onClose={handleSettingsToggle} pos={popoverPos} />

      {collapsed ? (
        /* Collapsed: just a status dot + gear icon */
        <div className="flex flex-col items-center gap-2">
          <span className={`h-2 w-2 rounded-full shrink-0 ${dotColor}`} title={statusLabel} />
          <button
            ref={settingsBtnRef}
            onClick={handleSettingsToggle}
            aria-label="Settings"
            title="Settings"
            className="flex h-9 w-9 items-center justify-center rounded-lg text-muted-foreground hover:bg-accent/70 hover:text-foreground transition-colors"
          >
            <Settings size={15} aria-hidden />
          </button>
        </div>
      ) : (
        /* Expanded: mode card + settings button */
        <>
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
                <span className={`h-2 w-2 rounded-full shrink-0 ${dotColor}`} />
                <span className={`text-[10px] ${textColor}`}>{statusLabel}</span>
              </div>
            </div>
          </div>

          <button
            ref={settingsBtnRef}
            onClick={handleSettingsToggle}
            className="flex items-center gap-2.5 rounded-lg px-2.5 py-2 text-sm font-medium text-muted-foreground hover:bg-accent/60 hover:text-foreground transition-colors w-full"
          >
            <Settings size={15} aria-hidden className="text-muted-foreground" />
            Settings
          </button>
        </>
      )}
    </div>
  );
}

/* --- Sidebar -------------------------------------------------------- */

export function Sidebar() {
  const path = usePathname();
  const router = useRouter();

  const [collapsed, setCollapsed] = useState(() => {
    if (typeof window !== "undefined") {
      return localStorage.getItem("sidebar-collapsed") === "true";
    }
    return false;
  });
  const [settingsOpen, setSettingsOpen] = useState(false);

  const toggleCollapse = () => {
    const next = !collapsed;
    setCollapsed(next);
    localStorage.setItem("sidebar-collapsed", String(next));
    if (next) setSettingsOpen(false);
  };

  // ⌘B / Ctrl+B — collapse/expand
  useEffect(() => {
    function onKey(e: KeyboardEvent) {
      if ((e.metaKey || e.ctrlKey) && e.key === "b") {
        e.preventDefault();
        toggleCollapse();
      }
    }
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [collapsed]);

  // ⌘K / Ctrl+K → search
  useEffect(() => {
    function onKey(e: KeyboardEvent) {
      if ((e.metaKey || e.ctrlKey) && e.key === "k") {
        e.preventDefault();
        router.push("/search");
      }
    }
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [router]);

  // Native app-menu "New Project" (⌘N) fires a custom DOM event.
  useEffect(() => {
    function onNativeNew() { setCreateOpen(true); }
    window.addEventListener("valori:new-project", onNativeNew);
    return () => window.removeEventListener("valori:new-project", onNativeNew);
  }, []);

  const { projects, isLoading, create, open } = useProjectManifest();
  const { isStandalone } = useCluster();
  const [createOpen, setCreateOpen] = useState(false);
  const [workspaceDir, setWorkspaceDir] = useState<string | null>(null);

  useEffect(() => {
    if (nativeAvailable()) {
      getPreference<string>("workspaceDir").then(setWorkspaceDir).catch(() => {});
    }
  }, []);

  const isActive = (href: string) =>
    path === href || (href !== "/" && path.startsWith(href + "/"));

  return (
    <>
      <aside
        className={cn(
          "flex h-screen flex-col border-r border-border/80 bg-card flex-shrink-0 transition-[width] duration-200 overflow-hidden",
          collapsed ? "w-[52px]" : "w-56"
        )}
        aria-label="Valori application navigation sidebar"
      >

        {/* Logo + collapse toggle — h-11 matches the TopBar height exactly */}
        <div className={cn("h-11 shrink-0 border-b border-border/80 flex items-center", collapsed ? "px-1.5 justify-center" : "px-4 justify-between")}>
          {collapsed ? (
            <button
              onClick={toggleCollapse}
              title="Expand sidebar (⌘B)"
              aria-label="Expand sidebar"
              className="flex items-center justify-center rounded-lg hover:bg-accent/60 transition-colors p-1"
            >
              <Image
                src="/logo.png"
                alt="Valori"
                width={22}
                height={22}
                className="dark:invert"
              />
            </button>
          ) : (
            <>
              <Link href="/" className="flex items-center gap-2">
                <Image
                  src="/logo.png"
                  alt="Valori"
                  width={24}
                  height={24}
                  className="dark:invert"
                />
                <div className="flex items-baseline gap-1">
                  <span className="font-mono text-sm font-bold tracking-tight text-foreground">valori</span>
                  <span className="font-mono text-[10px] text-muted-foreground">kernel</span>
                </div>
              </Link>
              <button
                onClick={toggleCollapse}
                title="Collapse sidebar (⌘B)"
                aria-label="Collapse sidebar"
                className="flex h-6 w-6 items-center justify-center rounded-md border border-border/60 text-muted-foreground hover:bg-accent/60 hover:text-foreground transition-colors"
              >
                <ChevronLeft size={12} aria-hidden />
              </button>
            </>
          )}
        </div>

        {/* Scrollable nav */}
        <div className={cn("flex-1 overflow-y-auto pb-2", collapsed ? "px-1" : "px-2")}>

          {/* Top nav */}
          <nav className="flex flex-col gap-0.5 pt-2">
            <NavLink item={{ href: "/", label: "Workspace", Icon: Home }} active={path === "/"} collapsed={collapsed} />
            {!isStandalone && (
              <NavLink item={{ href: "/cluster", label: "Cluster", Icon: Network }} active={isActive("/cluster")} collapsed={collapsed} />
            )}
            <NavLink item={{ href: "/operations", label: "Operations", Icon: Activity }} active={isActive("/operations")} collapsed={collapsed} />
            <NavLink item={{ href: "/metrics",    label: "Metrics",    Icon: BarChart2 }} active={isActive("/metrics")} collapsed={collapsed} />
            <NavLink item={{ href: "/proof",      label: "Proof",      Icon: ShieldCheck }} active={isActive("/proof")} collapsed={collapsed} />
            <NavLink item={{ href: "/audit",      label: "Audit Trail", Icon: ScrollText }} active={isActive("/audit")} collapsed={collapsed} />
            <NavLink item={{ href: "/launch",     label: "Launch",     Icon: Rocket }} active={isActive("/launch")} collapsed={collapsed} />
            <NavLink item={{ href: "/playground", label: "Playground", Icon: SquareTerminal }} active={isActive("/playground")} collapsed={collapsed} />
          </nav>

          {/* Search hint — hidden when collapsed */}
          {!collapsed && (
            <button
              onClick={() => router.push("/search")}
              className="mt-2 w-full flex items-center gap-2 rounded-lg border border-border/60 bg-background px-2.5 py-1.5 text-xs text-muted-foreground hover:text-foreground hover:border-border transition-colors"
            >
              <Search size={12} className="shrink-0" />
              <span className="flex-1 text-left">Search vectors…</span>
              <kbd className="text-[9px] border border-border/60 rounded px-1 py-0.5 font-mono bg-accent">⌘K</kbd>
            </button>
          )}

          {/* Divider */}
          <div className={cn("border-t border-border/80", collapsed ? "mx-1 my-2" : "mx-1 my-3")} />

          {/* Projects */}
          {!collapsed && (
            <div className="flex items-center justify-between px-2.5 mb-1.5">
              <p className="text-[11px] font-semibold uppercase tracking-[0.10em] text-muted-foreground select-none">
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
          )}

          {collapsed && (
            <button
              onClick={() => setCreateOpen(true)}
              title="New project"
              aria-label="New project"
              className="flex h-9 w-9 mx-auto items-center justify-center rounded-lg text-muted-foreground hover:bg-accent/60 hover:text-foreground transition-colors mb-1"
            >
              <Plus size={14} aria-hidden />
            </button>
          )}

          <div className="flex flex-col gap-0.5">
            {isLoading ? (
              <div className="flex flex-col gap-1.5 px-2 pt-1">
                {[1, 2, 3].map((i) => (
                  <div key={i} className={cn("animate-pulse rounded-lg bg-accent/60", collapsed ? "h-9 w-9 mx-auto" : "h-7")} />
                ))}
              </div>
            ) : projects.length === 0 ? (
              !collapsed && (
                <button
                  onClick={() => setCreateOpen(true)}
                  className="mx-1 mt-1 flex items-center justify-center gap-1.5 rounded-lg border border-dashed border-border py-3 text-xs text-muted-foreground hover:border-muted hover:text-muted-foreground transition-colors"
                >
                  <Plus size={12} />
                  Create first project
                </button>
              )
            ) : (
              projects.map((p) => {
                const href = `/projects/${encodeURIComponent(p.name)}`;
                const active = path === href || path.startsWith(href + "/");
                const running = p.status === "running" || p.status === "starting";
                const cols = p.collections || [];

                if (collapsed) {
                  return (
                    <Link
                      key={p.name}
                      href={href}
                      title={p.name}
                      aria-label={p.name}
                      className={cn(
                        "relative flex h-9 w-9 mx-auto items-center justify-center rounded-lg transition-all duration-150",
                        active
                          ? "bg-[var(--v-accent-muted)] text-[var(--v-accent)] [box-shadow:inset_2px_0_0_var(--v-accent)]"
                          : "text-muted-foreground hover:bg-accent/60 hover:text-foreground"
                      )}
                    >
                      <Layers size={14} aria-hidden />
                      <span
                        className={cn("absolute top-1.5 right-1.5 h-1.5 w-1.5 rounded-full", running ? "bg-emerald-400" : "bg-muted-foreground/30")}
                      />
                    </Link>
                  );
                }

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
                          className={active ? "text-[var(--v-accent)]" : "text-muted-foreground group-hover:text-muted-foreground"}
                        />
                        <span className="truncate">{p.name}</span>
                      </span>
                      <span
                        className={cn("ml-1 h-2 w-2 rounded-full shrink-0", running ? "bg-emerald-400" : "bg-muted-foreground/40")}
                        title={p.nodesTotal > 1 ? `${p.nodesRunning}/${p.nodesTotal} nodes running` : running ? "running" : "at rest"}
                      />
                    </Link>

                    {active && cols.length > 0 && (
                      <div className="ml-4 mt-0.5 flex flex-col gap-0.5 border-l border-border pl-2">
                        {cols.map((col) => {
                          const colHref = `${href}/${encodeURIComponent(col)}`;
                          return (
                            <Link
                              key={col}
                              href={colHref}
                              className={cn(
                                "flex items-center gap-1.5 rounded-md px-2 py-2 text-[11px] transition-all duration-150 truncate",
                                path === colHref
                                  ? "bg-[var(--v-accent-muted)] text-foreground"
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

        <StatusFooter
          collapsed={collapsed}
          settingsOpen={settingsOpen}
          onSettingsToggle={() => setSettingsOpen((o) => !o)}
        />
      </aside>

      <CreateProjectDialog
        open={createOpen}
        onClose={() => setCreateOpen(false)}
        workspaceDir={workspaceDir}
        onCreate={async (name, dim, index, replication, shardCount, embed) => {
          const entry = await create({ name, dim, index, replication, shardCount, embed });
          if (!entry) return;
          await open(name);
          router.push(`/projects/${encodeURIComponent(name)}`);
        }}
      />
    </>
  );
}
