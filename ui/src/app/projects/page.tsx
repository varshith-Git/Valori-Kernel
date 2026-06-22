"use client";

import { useState } from "react";
import { useRouter } from "next/navigation";
import { Plus } from "lucide-react";
import { useProjectGroups } from "@/lib/hooks/useCollections";
import { useProjects } from "@/lib/hooks/useProjects";
import { CreateProjectDialog } from "@/components/projects/CreateProjectDialog";
import { DeleteProjectDialog } from "@/components/projects/DeleteProjectDialog";
import { Button } from "@/components/ui/button";

export default function ProjectsPage() {
  const router = useRouter();
  const { groups, isLoading } = useProjectGroups();
  const { create, drop } = useProjects();
  const [createOpen, setCreateOpen] = useState(false);
  const [deleteTarget, setDeleteTarget] = useState<string | null>(null);

  if (isLoading) {
    return (
      <div className="flex flex-col gap-4 max-w-5xl">
        <div className="h-7 w-32 animate-pulse rounded bg-accent" />
        <div className="grid grid-cols-3 gap-4">
          {[1, 2, 3].map((i) => (
            <div key={i} className="h-28 animate-pulse rounded-xl bg-accent" />
          ))}
        </div>
      </div>
    );
  }

  return (
    <>
      <div className="flex flex-col gap-6 max-w-5xl">
        <div className="flex items-center justify-between">
          <div>
            <h1 className="text-xl font-semibold text-foreground">Projects</h1>
            <p className="mt-1 text-sm text-muted-foreground">
              Each project groups collections. Collections are stored as{" "}
              <code className="font-mono text-muted-foreground">project--collection</code> namespaces in Valori.
            </p>
          </div>
          <Button
            onClick={() => setCreateOpen(true)}
            size="sm"
            className="bg-[var(--v-accent)] text-white hover:opacity-90 gap-1.5"
          >
            <Plus size={14} />
            New Project
          </Button>
        </div>

        {groups.length === 0 ? (
          <div className="rounded-xl border border-dashed border-border py-20 text-center flex flex-col items-center gap-4">
            <div className="h-12 w-12 rounded-xl bg-card border border-border flex items-center justify-center">
              <Plus size={20} className="text-muted-foreground" />
            </div>
            <div>
              <p className="text-sm font-medium text-muted-foreground">No projects yet</p>
              <p className="mt-1 text-xs text-muted-foreground">Create a project, then add collections inside it.</p>
            </div>
            <Button
              onClick={() => setCreateOpen(true)}
              size="sm"
              variant="outline"
              className="border-input text-accent-foreground hover:bg-accent gap-1.5"
            >
              <Plus size={13} />
              Create project
            </Button>
          </div>
        ) : (
          <div className="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 gap-4">
            {groups.map((g) => (
              <ProjectCard
                key={g.project}
                project={g.project}
                collectionCount={g.collections.length}
                isBare={g.isBare}
                onClick={() =>
                  router.push(`/projects/${encodeURIComponent(g.project)}`)
                }
                onDelete={() => setDeleteTarget(g.project)}
              />
            ))}
            <button
              onClick={() => setCreateOpen(true)}
              className="flex items-center justify-center gap-2 rounded-xl border border-dashed border-border py-8 text-sm text-muted-foreground hover:border-[var(--v-accent)] hover:text-muted-foreground transition-colors group"
            >
              <Plus size={16} className="group-hover:text-[var(--v-accent)] transition-colors" />
              New Project
            </button>
          </div>
        )}
      </div>

      <CreateProjectDialog
        open={createOpen}
        onClose={() => setCreateOpen(false)}
        onCreate={async (name: string) => {
          // Projects are implicit — we just navigate to the new project page
          // where the user will create the first collection.
          router.push(`/projects/${encodeURIComponent(name)}`);
        }}
      />

      {deleteTarget && (
        <DeleteProjectDialog
          name={deleteTarget}
          open={true}
          onClose={() => setDeleteTarget(null)}
          onDelete={async () => {
            // Drop all collections belonging to this project
            // (namespaces starting with `{project}--`)
            // For bare projects, drop the namespace itself.
            const group = groups.find((g) => g.project === deleteTarget);
            if (!group) return;
            if (group.isBare) {
              await drop(deleteTarget);
            } else {
              for (const col of group.collections) {
                await drop(`${deleteTarget}--${col}`);
              }
            }
            setDeleteTarget(null);
          }}
        />
      )}
    </>
  );
}

function ProjectCard({
  project,
  collectionCount,
  isBare,
  onClick,
  onDelete,
}: {
  project: string;
  collectionCount: number;
  isBare: boolean;
  onClick: () => void;
  onDelete: () => void;
}) {
  return (
    <div
      className="group relative rounded-xl border border-border bg-card p-5 cursor-pointer hover:border-input transition-colors"
      onClick={onClick}
    >
      <div className="flex items-start justify-between">
        <div className="flex items-center gap-2">
          <span className="text-muted-foreground font-mono text-sm">⬡</span>
          {isBare && (
            <span className="text-[10px] text-muted-foreground border border-border rounded px-1.5 py-0.5">
              legacy
            </span>
          )}
        </div>
        <button
          onClick={(e) => {
            e.stopPropagation();
            onDelete();
          }}
          className="opacity-0 group-hover:opacity-100 text-xs text-muted-foreground hover:text-red-400 transition-all"
        >
          delete
        </button>
      </div>

      <p className="mt-3 font-medium text-foreground text-base truncate">{project}</p>

      <div className="mt-2 flex items-center gap-1.5">
        <span className="text-xs text-muted-foreground">
          {isBare ? (
            "bare namespace"
          ) : collectionCount === 0 ? (
            <span className="text-muted-foreground">no collections</span>
          ) : (
            `${collectionCount} collection${collectionCount !== 1 ? "s" : ""}`
          )}
        </span>
      </div>
    </div>
  );
}
