"use client";

import { useState } from "react";
import { useRouter } from "next/navigation";
import { Folder, Plus, Search, RefreshCw } from "lucide-react";
import { useProjectManifest } from "@/lib/hooks/useProjectManifest";
import { forgetProject } from "@/lib/native";
import { ProjectCard } from "@/components/projects/ProjectCard";
import { CreateProjectDialog } from "@/components/projects/CreateProjectDialog";
import { DeleteProjectDialog } from "@/components/projects/DeleteProjectDialog";
import { LocalRenameDialog } from "@/components/projects/LocalRenameDialog";
import { ProjectModePicker } from "@/components/projects/ProjectModePicker";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Skeleton } from "@/components/ui/skeleton";

export default function ProjectsOverviewPage() {
  const router = useRouter();
  const { projects, isLoading, create, open, rename, remove, refresh } = useProjectManifest();
  const [filter, setFilter] = useState("");
  const [pickerOpen, setPickerOpen] = useState(false);
  const [createOpen, setCreateOpen] = useState(false);
  const [deleteTarget, setDeleteTarget] = useState<string | null>(null);
  const [renameTarget, setRenameTarget] = useState<string | null>(null);

  const filteredProjects = projects.filter((p) =>
    p.name.toLowerCase().includes(filter.toLowerCase().trim())
  );

  return (
    <div className="flex flex-col gap-6 w-full max-w-[1600px]">
      {/* Header */}
      <div className="flex flex-col sm:flex-row sm:items-center justify-between gap-4">
        <div>
          <h1 className="text-xl font-semibold tracking-tight text-foreground flex items-center gap-2">
            <Folder className="h-5 w-5 text-primary" />
            Projects
          </h1>
          <p className="text-xs text-muted-foreground mt-1">
            Manage your vector storage projects, collections, and nodes
          </p>
        </div>

        <div className="flex items-center gap-2">
          <Button
            size="sm"
            variant="outline"
            onClick={() => refresh()}
            className="h-9 gap-1.5 text-xs border-input"
          >
            <RefreshCw size={13} className={isLoading ? "animate-spin" : ""} />
            Refresh
          </Button>
          <Button
            size="sm"
            onClick={() => setPickerOpen(true)}
            className="h-9 gap-1.5 text-xs bg-primary text-primary-foreground hover:bg-primary/90"
          >
            <Plus size={14} />
            New project
          </Button>
        </div>
      </div>

      {/* Search Filter */}
      <div className="relative max-w-sm">
        <Search size={14} className="absolute left-3 top-1/2 -translate-y-1/2 text-muted-foreground" />
        <Input
          placeholder="Filter projects by name…"
          value={filter}
          onChange={(e) => setFilter(e.target.value)}
          className="pl-9 h-9 text-xs bg-card border-input"
        />
      </div>

      {/* Projects Grid */}
      {isLoading && projects.length === 0 ? (
        <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-5">
          {[1, 2, 3].map((i) => (
            <Skeleton key={i} className="h-56 rounded-2xl bg-accent/60" />
          ))}
        </div>
      ) : projects.length === 0 ? (
        <div className="rounded-2xl border border-dashed border-border bg-card p-12 text-center flex flex-col items-center justify-center gap-3">
          <div className="h-12 w-12 rounded-full bg-accent/80 flex items-center justify-center text-muted-foreground">
            <Folder size={24} />
          </div>
          <div>
            <h3 className="text-sm font-medium text-foreground">No projects found</h3>
            <p className="text-xs text-muted-foreground mt-1 max-w-sm">
              Create your first vector storage project to organize collections and search vectors.
            </p>
          </div>
          <Button
            size="sm"
            onClick={() => setPickerOpen(true)}
            className="mt-2 h-8 text-xs gap-1.5"
          >
            <Plus size={13} />
            Create project
          </Button>
        </div>
      ) : (
        <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-5">
          {filteredProjects.map((p) => (
            <ProjectCard
              key={p.name}
              data={{
                kind: "local",
                name: p.name,
                status: p.status,
                port: p.port,
                nodesRunning: p.nodesRunning,
                nodesTotal: p.nodesTotal,
                shardCount: p.shardCount,
                records: p.records,
                collections: p.collections,
                href: `/projects/${encodeURIComponent(p.name)}`,
              }}
              onRename={() => setRenameTarget(p.name)}
              onDelete={() => setDeleteTarget(p.name)}
            />
          ))}
        </div>
      )}

      {/* Dialogs */}
      <ProjectModePicker
        open={pickerOpen}
        onClose={() => setPickerOpen(false)}
        onLocal={() => setCreateOpen(true)}
      />

      <CreateProjectDialog
        open={createOpen}
        onClose={() => setCreateOpen(false)}
        onCreate={async (name, replication, shardCount) => {
          const entry = await create({ name, replication, shardCount });
          if (!entry) return;
          const ok = await open(name);
          if (ok) router.push(`/projects/${encodeURIComponent(name)}`);
        }}
      />

      {deleteTarget && (
        <DeleteProjectDialog
          name={deleteTarget}
          open={Boolean(deleteTarget)}
          onClose={() => setDeleteTarget(null)}
          onDelete={async () => {
            await remove(deleteTarget);
            forgetProject(deleteTarget).catch(() => {});
            setDeleteTarget(null);
          }}
        />
      )}

      {renameTarget && (
        <LocalRenameDialog
          name={renameTarget}
          onClose={() => setRenameTarget(null)}
          onSuccess={async (newName) => {
            await rename(renameTarget, newName);
            setRenameTarget(null);
          }}
        />
      )}
    </div>
  );
}
