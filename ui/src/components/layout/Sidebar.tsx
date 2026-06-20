"use client";

import Link from "next/link";
import { usePathname, useRouter } from "next/navigation";
import { useState } from "react";
import { cn } from "@/lib/utils";
import { useCluster } from "@/lib/hooks/useCluster";
import { useProjectGroups } from "@/lib/hooks/useCollections";
import { CreateProjectDialog } from "@/components/projects/CreateProjectDialog";

const GLOBAL_NAV = [
  { href: "/", label: "Proof", icon: "◆" },
  { href: "/search", label: "Search", icon: "⊙" },
  { href: "/audit", label: "Audit Trail", icon: "≡" },
];

export function Sidebar() {
  const path = usePathname();
  const router = useRouter();
  const { groups, isLoading } = useProjectGroups();
  const { isStandalone } = useCluster();
  const [createOpen, setCreateOpen] = useState(false);

  return (
    <>
      <aside className="flex h-screen w-52 flex-col border-r border-zinc-800 bg-zinc-950 flex-shrink-0">
        {/* Logo */}
        <div className="px-4 py-5 border-b border-zinc-800">
          <Link href="/" className="flex items-baseline gap-1">
            <span className="font-mono text-sm font-semibold tracking-tight text-white">valori</span>
            <span className="font-mono text-xs text-zinc-500">kernel</span>
          </Link>
        </div>

        {/* Global nav */}
        <nav className="flex flex-col gap-0.5 px-2 pt-3">
          {GLOBAL_NAV.map((n) => (
            <Link
              key={n.href}
              href={n.href}
              className={cn(
                "flex items-center gap-2 rounded-md px-2 py-2 text-sm transition-colors",
                path === n.href
                  ? "bg-zinc-800 text-white"
                  : "text-zinc-400 hover:bg-zinc-900 hover:text-zinc-100"
              )}
            >
              <span className="w-4 text-center font-mono text-xs opacity-70">{n.icon}</span>
              {n.label}
            </Link>
          ))}
          {!isStandalone && (
            <Link
              href="/cluster"
              className={cn(
                "flex items-center gap-2 rounded-md px-2 py-2 text-sm transition-colors",
                path === "/cluster"
                  ? "bg-zinc-800 text-white"
                  : "text-zinc-400 hover:bg-zinc-900 hover:text-zinc-100"
              )}
            >
              <span className="w-4 text-center font-mono text-xs opacity-70">⬡</span>
              Cluster
            </Link>
          )}
        </nav>

        {/* Divider */}
        <div className="mx-3 my-3 border-t border-zinc-800" />

        {/* Projects + Collections section */}
        <div className="flex items-center justify-between px-3 mb-1.5">
          <span className="text-[10px] font-medium uppercase tracking-widest text-zinc-600">
            Projects
          </span>
          <button
            onClick={() => setCreateOpen(true)}
            className="text-[10px] text-zinc-600 hover:text-zinc-300 transition-colors"
            title="New project"
          >
            + new
          </button>
        </div>

        <div className="flex flex-col gap-0.5 px-2 overflow-y-auto flex-1 pb-3">
          {isLoading ? (
            <div className="flex flex-col gap-1 px-2 pt-1">
              {[1, 2, 3].map((i) => (
                <div key={i} className="h-7 animate-pulse rounded bg-zinc-900" />
              ))}
            </div>
          ) : groups.length === 0 ? (
            <button
              onClick={() => setCreateOpen(true)}
              className="mx-1 mt-1 rounded-md border border-dashed border-zinc-800 py-3 text-xs text-zinc-600 hover:border-zinc-600 hover:text-zinc-400 transition-colors"
            >
              + Create first project
            </button>
          ) : (
            groups.map((g) => {
              const href = `/projects/${encodeURIComponent(g.project)}`;
              const active = path === href || path.startsWith(href + "/");
              return (
                <div key={g.project}>
                  {/* Project row */}
                  <Link
                    href={href}
                    className={cn(
                      "flex items-center justify-between rounded-md px-2 py-1.5 text-xs transition-colors",
                      active
                        ? "bg-zinc-800 text-white"
                        : "text-zinc-400 hover:bg-zinc-900 hover:text-zinc-200"
                    )}
                  >
                    <span className="flex items-center gap-1.5 truncate">
                      <span className="font-mono text-[10px] opacity-60">⬡</span>
                      <span className="truncate">{g.project}</span>
                    </span>
                    {!g.isBare && g.collections.length > 0 && (
                      <span className="ml-1 text-[10px] text-zinc-600 tabular-nums">
                        {g.collections.length}
                      </span>
                    )}
                  </Link>

                  {/* Inline collections under active project */}
                  {active && !g.isBare && g.collections.length > 0 && (
                    <div className="ml-4 mt-0.5 flex flex-col gap-0.5">
                      {g.collections.map((col) => {
                        const colHref = `${href}/${encodeURIComponent(col)}`;
                        return (
                          <Link
                            key={col}
                            href={colHref}
                            className={cn(
                              "flex items-center gap-1.5 rounded-md px-2 py-1.5 text-[11px] transition-colors truncate",
                              path === colHref
                                ? "bg-zinc-800 text-white"
                                : "text-zinc-500 hover:bg-zinc-900 hover:text-zinc-300"
                            )}
                          >
                            <span className="font-mono text-[9px] opacity-50">⊞</span>
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

        {/* Footer */}
        <div className="px-3 py-3 border-t border-zinc-800">
          <Link
            href="/projects"
            className={cn(
              "flex items-center gap-2 rounded-md px-2 py-1.5 text-xs transition-colors",
              path === "/projects"
                ? "bg-zinc-800 text-white"
                : "text-zinc-600 hover:text-zinc-300"
            )}
          >
            <span className="text-[10px]">⊞</span>
            All projects
          </Link>
        </div>
      </aside>

      <CreateProjectDialog
        open={createOpen}
        onClose={() => setCreateOpen(false)}
        onCreate={async (name: string) => {
          router.push(`/projects/${encodeURIComponent(name)}`);
        }}
      />
    </>
  );
}
