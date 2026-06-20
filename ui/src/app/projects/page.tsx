"use client";

import { useState } from "react";
import { useRouter } from "next/navigation";
import { useProjectGroups } from "@/lib/hooks/useCollections";
import { useProjects } from "@/lib/hooks/useProjects";
import { CreateProjectDialog } from "@/components/projects/CreateProjectDialog";
import { DeleteProjectDialog } from "@/components/projects/DeleteProjectDialog";

export default function ProjectsPage() {
  const router = useRouter();
  const { groups, isLoading } = useProjectGroups();
  const { create, drop } = useProjects();
  const [createOpen, setCreateOpen] = useState(false);
  const [deleteTarget, setDeleteTarget] = useState<string | null>(null);

  if (isLoading) {
    return (
      <div className="flex flex-col gap-4 max-w-5xl">
        <div className="h-7 w-32 animate-pulse rounded bg-zinc-800" />
        <div className="grid grid-cols-3 gap-4">
          {[1, 2, 3].map((i) => (
            <div key={i} className="h-28 animate-pulse rounded-xl bg-zinc-800" />
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
            <h1 className="text-xl font-semibold text-white">Projects</h1>
            <p className="mt-1 text-sm text-zinc-500">
              Each project groups collections. Collections are stored as{" "}
              <code className="font-mono text-zinc-400">project--collection</code> namespaces in Valori.
            </p>
          </div>
          <button
            onClick={() => setCreateOpen(true)}
            className="rounded-lg border border-zinc-700 bg-zinc-900 px-4 py-2 text-sm text-white hover:bg-zinc-800 transition-colors"
          >
            + New Project
          </button>
        </div>

        {groups.length === 0 ? (
          <div className="rounded-xl border border-dashed border-zinc-800 py-16 text-center">
            <p className="text-sm text-zinc-500">No projects yet.</p>
            <p className="mt-1 text-xs text-zinc-600">
              Create a project, then add collections inside it.
            </p>
            <button
              onClick={() => setCreateOpen(true)}
              className="mt-4 rounded-lg border border-zinc-700 px-4 py-2 text-sm text-zinc-300 hover:bg-zinc-800 transition-colors"
            >
              + New Project
            </button>
          </div>
        ) : (
          <div className="grid grid-cols-3 gap-4">
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
              className="flex items-center justify-center rounded-xl border border-dashed border-zinc-800 py-8 text-sm text-zinc-600 hover:border-zinc-600 hover:text-zinc-400 transition-colors"
            >
              + New Project
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
      className="group relative rounded-xl border border-zinc-800 bg-zinc-900 p-5 cursor-pointer hover:border-zinc-700 transition-colors"
      onClick={onClick}
    >
      <div className="flex items-start justify-between">
        <div className="flex items-center gap-2">
          <span className="text-zinc-600 font-mono text-sm">⬡</span>
          {isBare && (
            <span className="text-[10px] text-zinc-600 border border-zinc-800 rounded px-1.5 py-0.5">
              legacy
            </span>
          )}
        </div>
        <button
          onClick={(e) => {
            e.stopPropagation();
            onDelete();
          }}
          className="opacity-0 group-hover:opacity-100 text-xs text-zinc-600 hover:text-red-400 transition-all"
        >
          delete
        </button>
      </div>

      <p className="mt-3 font-medium text-white text-base truncate">{project}</p>

      <div className="mt-2 flex items-center gap-1.5">
        <span className="text-xs text-zinc-500">
          {isBare ? (
            "bare namespace"
          ) : collectionCount === 0 ? (
            <span className="text-zinc-600">no collections</span>
          ) : (
            `${collectionCount} collection${collectionCount !== 1 ? "s" : ""}`
          )}
        </span>
      </div>
    </div>
  );
}
