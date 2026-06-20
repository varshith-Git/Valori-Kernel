"use client";

import { useState } from "react";
import { useRouter } from "next/navigation";
import { useProjects } from "@/lib/hooks/useProjects";
import { CreateProjectDialog } from "@/components/projects/CreateProjectDialog";
import { DeleteProjectDialog } from "@/components/projects/DeleteProjectDialog";

export default function ProjectsPage() {
  const router = useRouter();
  const { projects, isLoading, create, drop } = useProjects();
  const [createOpen, setCreateOpen] = useState(false);
  const [deleteTarget, setDeleteTarget] = useState<string | null>(null);

  if (isLoading) {
    return (
      <div className="flex flex-col gap-4 max-w-4xl">
        <div className="h-7 w-32 animate-pulse rounded bg-zinc-800" />
        <div className="grid grid-cols-3 gap-4">
          {[1, 2, 3].map((i) => (
            <div key={i} className="h-24 animate-pulse rounded-xl bg-zinc-800" />
          ))}
        </div>
      </div>
    );
  }

  return (
    <>
      <div className="flex flex-col gap-6 max-w-4xl">
        <div className="flex items-center justify-between">
          <div>
            <h1 className="text-xl font-semibold text-white">Projects</h1>
            <p className="mt-1 text-sm text-zinc-500">
              Each project is an isolated vector store (Valori namespace)
            </p>
          </div>
          <button
            onClick={() => setCreateOpen(true)}
            className="rounded-lg border border-zinc-700 bg-zinc-900 px-4 py-2 text-sm text-white hover:bg-zinc-800 transition-colors"
          >
            + New Project
          </button>
        </div>

        {projects.length === 0 ? (
          <div className="rounded-xl border border-dashed border-zinc-800 py-16 text-center">
            <p className="text-sm text-zinc-500">No projects yet.</p>
            <p className="mt-1 text-xs text-zinc-600">
              Create one to start storing vectors.
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
            {projects.map((name) => (
              <ProjectCard
                key={name}
                name={name}
                onClick={() => router.push(`/projects/${encodeURIComponent(name)}`)}
                onDelete={() => setDeleteTarget(name)}
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
        onCreate={create}
      />

      {deleteTarget && (
        <DeleteProjectDialog
          name={deleteTarget}
          open={true}
          onClose={() => setDeleteTarget(null)}
          onDelete={() => drop(deleteTarget)}
        />
      )}
    </>
  );
}

function ProjectCard({
  name,
  onClick,
  onDelete,
}: {
  name: string;
  onClick: () => void;
  onDelete: () => void;
}) {
  return (
    <div
      className="group relative rounded-xl border border-zinc-800 bg-zinc-900 p-5 cursor-pointer hover:border-zinc-600 transition-colors"
      onClick={onClick}
    >
      <div className="flex items-start justify-between">
        <span className="text-base">📁</span>
        <button
          onClick={(e) => { e.stopPropagation(); onDelete(); }}
          className="opacity-0 group-hover:opacity-100 text-xs text-zinc-600 hover:text-red-400 transition-all px-1"
        >
          delete
        </button>
      </div>
      <p className="mt-2 font-medium text-white text-sm truncate">{name}</p>
      <p className="mt-0.5 text-xs text-zinc-500">Click to open</p>
    </div>
  );
}
