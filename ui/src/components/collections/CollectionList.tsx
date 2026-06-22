"use client";

import { useState } from "react";
import Link from "next/link";
import { Button } from "@/components/ui/button";
import { CreateCollectionDialog } from "./CreateCollectionDialog";
import { DeleteCollectionDialog } from "./DeleteCollectionDialog";

interface Props {
  project: string;
  collections: string[];
  isLoading: boolean;
  onCreate: (name: string) => Promise<void>;
  onDrop: (name: string) => Promise<void>;
}

export function CollectionList({
  project,
  collections,
  isLoading,
  onCreate,
  onDrop,
}: Props) {
  const [createOpen, setCreateOpen] = useState(false);
  const [deleteTarget, setDeleteTarget] = useState<string | null>(null);

  return (
    <div className="flex flex-col gap-4">
      {/* Header row */}
      <div className="flex items-center justify-between">
        <p className="text-sm text-muted-foreground">
          {collections.length} collection{collections.length !== 1 ? "s" : ""}
        </p>
        <Button
          size="sm"
          onClick={() => setCreateOpen(true)}
          className="bg-primary text-primary-foreground hover:bg-primary/90 h-8 text-xs px-3"
        >
          + New collection
        </Button>
      </div>

      {/* Collection grid */}
      {isLoading ? (
        <div className="grid grid-cols-3 gap-3">
          {[1, 2, 3].map((i) => (
            <div key={i} className="h-24 animate-pulse rounded-xl bg-accent" />
          ))}
        </div>
      ) : collections.length === 0 ? (
        <div className="rounded-xl border border-dashed border-border py-14 text-center">
          <p className="text-sm text-muted-foreground">No collections yet</p>
          <p className="mt-1 text-xs text-muted-foreground">
            Create your first collection to start inserting vectors.
          </p>
          <Button
            size="sm"
            onClick={() => setCreateOpen(true)}
            className="mt-4 bg-primary text-primary-foreground hover:bg-primary/90 h-8 text-xs"
          >
            + New collection
          </Button>
        </div>
      ) : (
        <div className="grid grid-cols-3 gap-3">
          {collections.map((col) => (
            <CollectionCard
              key={col}
              project={project}
              collection={col}
              onDelete={() => setDeleteTarget(col)}
            />
          ))}
        </div>
      )}

      <CreateCollectionDialog
        project={project}
        open={createOpen}
        onOpenChange={setCreateOpen}
        onCreate={onCreate}
      />

      {deleteTarget && (
        <DeleteCollectionDialog
          project={project}
          collection={deleteTarget}
          open={!!deleteTarget}
          onOpenChange={(o) => !o && setDeleteTarget(null)}
          onDelete={async () => {
            await onDrop(deleteTarget);
            setDeleteTarget(null);
          }}
        />
      )}
    </div>
  );
}

function CollectionCard({
  project,
  collection,
  onDelete,
}: {
  project: string;
  collection: string;
  onDelete: () => void;
}) {
  const [hovered, setHovered] = useState(false);

  return (
    <div
      className="relative group rounded-xl border border-border bg-card hover:border-input transition-colors"
      onMouseEnter={() => setHovered(true)}
      onMouseLeave={() => setHovered(false)}
    >
      <Link
        href={`/projects/${encodeURIComponent(project)}/${encodeURIComponent(collection)}`}
        className="block p-5"
      >
        <div className="flex items-center gap-2 mb-2">
          <span className="text-muted-foreground font-mono text-xs">⊞</span>
          <span className="font-medium text-foreground text-sm truncate">{collection}</span>
        </div>
        <p className="text-[10px] text-muted-foreground font-mono truncate">
          {project}--{collection}
        </p>
      </Link>

      {hovered && (
        <button
          onClick={(e) => {
            e.preventDefault();
            onDelete();
          }}
          className="absolute top-3 right-3 rounded-md px-2 py-1 text-xs bg-accent text-muted-foreground hover:bg-red-950 hover:text-red-400 border border-input hover:border-red-900 transition-colors"
        >
          Delete
        </button>
      )}
    </div>
  );
}
