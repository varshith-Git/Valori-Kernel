import { cn } from "@/lib/utils";

/** A loading placeholder — one shape, reused instead of every page inventing
 *  its own `animate-pulse` div with slightly different radius/opacity. */
export function Skeleton({ className }: { className?: string }) {
  return <div className={cn("animate-pulse rounded-lg bg-muted/60", className)} />;
}

/** Common composite: a page-header-shaped skeleton, for the "we don't know
 *  the title yet" loading moment (e.g. fetching an operation/project by id). */
export function PageSkeleton() {
  return (
    <div className="flex w-full max-w-[1400px] flex-col gap-6">
      <Skeleton className="h-8 w-40" />
      <Skeleton className="h-32 w-full rounded-2xl" />
      <Skeleton className="h-96 w-full rounded-2xl" />
    </div>
  );
}
