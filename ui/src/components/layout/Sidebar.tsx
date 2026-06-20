"use client";

import Link from "next/link";
import { usePathname } from "next/navigation";
import { cn } from "@/lib/utils";

const NAV = [
  { href: "/", label: "Proof", icon: "◆" },
  { href: "/audit", label: "Audit Trail", icon: "≡" },
  { href: "/search", label: "Search", icon: "⊙" },
  { href: "/collections", label: "Collections", icon: "⊞" },
  { href: "/cluster", label: "Cluster", icon: "⬡", clusterOnly: true },
];

const CLUSTER_MODE = process.env.NEXT_PUBLIC_CLUSTER_MODE === "true";

export function Sidebar() {
  const path = usePathname();
  const links = NAV.filter((n) => !n.clusterOnly || CLUSTER_MODE);

  return (
    <aside className="flex h-screen w-52 flex-col border-r border-zinc-800 bg-zinc-950 px-3 py-6">
      <div className="mb-8 px-2">
        <span className="font-mono text-base font-semibold tracking-tight text-white">
          valori
        </span>
        <span className="ml-1 font-mono text-xs text-zinc-500">audit</span>
      </div>

      <nav className="flex flex-col gap-1">
        {links.map((n) => (
          <Link
            key={n.href}
            href={n.href}
            className={cn(
              "flex items-center gap-2.5 rounded-md px-2 py-2 text-sm transition-colors",
              path === n.href
                ? "bg-zinc-800 text-white"
                : "text-zinc-400 hover:bg-zinc-900 hover:text-zinc-100"
            )}
          >
            <span className="w-4 text-center font-mono text-xs">{n.icon}</span>
            {n.label}
          </Link>
        ))}
      </nav>

      <div className="mt-auto px-2 text-[10px] text-zinc-600">
        valori-kernel · Q16.16
      </div>
    </aside>
  );
}
