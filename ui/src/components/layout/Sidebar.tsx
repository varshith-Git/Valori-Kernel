"use client";

import Link from "next/link";
import { usePathname, useRouter } from "next/navigation";
import { useState } from "react";
import { cn } from "@/lib/utils";
import { useProjects } from "@/lib/hooks/useProjects";
import { useCluster } from "@/lib/hooks/useCluster";
import { CreateProjectDialog } from "@/components/projects/CreateProjectDialog";

const GLOBAL_NAV = [
  { href: "/", label: "Proof", icon: "◆" },
  { href: "/search", label: "Search", icon: "⊙" },
  { href: "/audit", label: "Audit Trail", icon: "≡" },
];

export function Sidebar() {
  const path = usePathname();
  const router = useRouter();
  const { projects, create, isLoading } = useProjects();
  const { isStandalone } = useCluster();
  const [createOpen, setCreateOpen] = useState(false);

  const handleCreate = async (name: string) => {
    await create(name);
    router.push(`/projects/${encodeURIComponent(name)}`);
  };

  return (
    <>
      <aside className="flex h-screen w-52 flex-col border-r border-zinc-800 bg-zinc-950 flex-shrink-0">
        {/* Logo */}
        <div className="px-4 py-5 border-b border-zinc-800">
          <Link href="/" className="flex items-baseline gap-1">
            <span className="font-mono text-sm font-semibold tracking-tight text-white">valori</span>
            <span className="font-mono text-xs text-zinc-500">audit</span>
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
          {/* Cluster link — shown when not standalone (auto-detected) */}
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

        {/* Projects section */}
        <div className="flex items-center justify-between px-3 mb-1.5">
          <span className="text-[10px] font-medium uppercase tracking-widest text-zinc-600">
            Projects
          </span>
          <button
            onClick={() => setCreateOpen(true)}
            className="text-[10px] text-zinc-600 hover:text-zinc-300 transition-colors leading-none"
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
          ) : projects.length === 0 ? (
            <button
              onClick={() => setCreateOpen(true)}
              className="mx-1 mt-1 rounded-md border border-dashed border-zinc-800 py-3 text-xs text-zinc-600 hover:border-zinc-600 hover:text-zinc-400 transition-colors"
            >
              + Create first project
            </button>
          ) : (
            projects.map((name) => {
              const href = `/projects/${encodeURIComponent(name)}`;
              const active = path === href || path.startsWith(href + "/");
              return (
                <Link
                  key={name}
                  href={href}
                  className={cn(
                    "flex items-center gap-2 rounded-md px-2 py-1.5 text-xs transition-colors truncate",
                    active
                      ? "bg-zinc-800 text-white"
                      : "text-zinc-500 hover:bg-zinc-900 hover:text-zinc-200"
                  )}
                >
                  <span className="text-[10px]">📁</span>
                  <span className="truncate">{name}</span>
                </Link>
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
        onCreate={handleCreate}
      />
    </>
  );
}
