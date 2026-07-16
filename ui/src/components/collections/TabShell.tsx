import { cn } from "@/lib/utils";

/**
 * Shared root layout for collection tab panels. Every tab was picking its
 * own max-width (2xl/3xl/4xl/md) and gap, so panels visibly resized as you
 * clicked between tabs. One width, one gap — wrap the tab's root div in
 * this instead of hardcoding `flex flex-col gap-N max-w-Nxl`.
 */
export function TabShell({ children, className }: { children: React.ReactNode; className?: string }) {
  return <div className={cn("flex flex-col gap-5 max-w-3xl", className)}>{children}</div>;
}
