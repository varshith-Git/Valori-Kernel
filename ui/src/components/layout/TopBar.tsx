"use client";

import { usePathname, useRouter } from "next/navigation";
import { ChevronLeft } from "lucide-react";
import { Breadcrumb } from "./Breadcrumb";

export function TopBar() {
  const path = usePathname();
  const router = useRouter();
  const isRoot = path === "/";

  return (
    <div className="h-11 shrink-0 border-b border-border/60 bg-background/80 backdrop-blur-sm flex items-center gap-1.5 px-5">
      <button
        onClick={() => router.back()}
        aria-label="Go back"
        className={`h-6 w-6 flex items-center justify-center rounded-md text-muted-foreground hover:text-foreground hover:bg-accent transition-all duration-150 ${
          isRoot ? "opacity-0 pointer-events-none" : "opacity-100"
        }`}
      >
        <ChevronLeft size={13} />
      </button>
      <Breadcrumb />
    </div>
  );
}
