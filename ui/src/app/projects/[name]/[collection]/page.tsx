"use client";

import { use } from "react";
import Link from "next/link";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { MultiSearch } from "@/components/collections/MultiSearch";
import { UploadTab } from "@/components/projects/UploadTab";
import { useHealth } from "@/lib/hooks/useHealth";
import { makeNs } from "@/lib/hooks/useCollections";

export default function CollectionPage({
  params,
}: {
  params: Promise<{ name: string; collection: string }>;
}) {
  const { name, collection } = use(params);
  const project = decodeURIComponent(name);
  const col = decodeURIComponent(collection);
  const namespace = makeNs(project, col);

  const { dim } = useHealth();

  const deleteRecord = async (id: number) => {
    await fetch("/api/delete", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ id }),
    });
  };

  return (
    <div className="flex flex-col gap-6 max-w-4xl">
      {/* Breadcrumb */}
      <div className="flex items-center gap-2 text-sm text-zinc-500">
        <Link href="/projects" className="hover:text-zinc-300 transition-colors">
          Projects
        </Link>
        <span>/</span>
        <Link
          href={`/projects/${encodeURIComponent(project)}`}
          className="hover:text-zinc-300 transition-colors"
        >
          {project}
        </Link>
        <span>/</span>
        <span className="text-white font-medium">{col}</span>
        <code className="ml-2 text-[10px] text-zinc-600 font-mono bg-zinc-900 px-2 py-0.5 rounded border border-zinc-800">
          ns: {namespace}
        </code>
      </div>

      <Tabs defaultValue="search">
        <TabsList className="bg-zinc-900 border border-zinc-800">
          <TabsTrigger
            value="search"
            className="data-[state=active]:bg-zinc-700 data-[state=active]:text-white text-zinc-400"
          >
            Search
          </TabsTrigger>
          <TabsTrigger
            value="upload"
            className="data-[state=active]:bg-zinc-700 data-[state=active]:text-white text-zinc-400"
          >
            Upload
          </TabsTrigger>
          <TabsTrigger
            value="info"
            className="data-[state=active]:bg-zinc-700 data-[state=active]:text-white text-zinc-400"
          >
            Info
          </TabsTrigger>
        </TabsList>

        <TabsContent value="search" className="mt-5">
          <MultiSearch
            namespace={namespace}
            dim={dim}
            onDelete={deleteRecord}
          />
        </TabsContent>

        <TabsContent value="upload" className="mt-5">
          <UploadTab collection={namespace} />
        </TabsContent>

        <TabsContent value="info" className="mt-5">
          <CollectionInfo project={project} collection={col} namespace={namespace} dim={dim} />
        </TabsContent>
      </Tabs>
    </div>
  );
}

function CollectionInfo({
  project,
  collection,
  namespace,
  dim,
}: {
  project: string;
  collection: string;
  namespace: string;
  dim: number | null;
}) {
  return (
    <div className="flex flex-col gap-4 max-w-md">
      <div className="rounded-xl border border-zinc-800 bg-zinc-900 divide-y divide-zinc-800">
        <InfoRow label="Project" value={project} />
        <InfoRow label="Collection" value={collection} />
        <InfoRow label="Namespace (Valori)" value={namespace} mono />
        <InfoRow label="Dimension" value={dim != null ? String(dim) : "—"} />
        <InfoRow
          label="Storage per record"
          value={dim != null ? `${dim * 4} bytes` : "—"}
          sub={dim != null ? `${dim} scalars × 4 B (Q16.16)` : undefined}
        />
        <InfoRow label="Search modes" value="Semantic · #id · Regex" />
        <InfoRow label="Pending modes" value="Text · Hybrid · Metadata" sub="requires embedding API" />
      </div>
    </div>
  );
}

function InfoRow({
  label,
  value,
  sub,
  mono,
}: {
  label: string;
  value: string;
  sub?: string;
  mono?: boolean;
}) {
  return (
    <div className="flex items-start justify-between px-4 py-3">
      <p className="text-sm text-zinc-400">{label}</p>
      <div className="text-right">
        <span className={`text-sm ${mono ? "font-mono" : ""} text-zinc-300`}>{value}</span>
        {sub && <p className="text-xs text-zinc-600 mt-0.5">{sub}</p>}
      </div>
    </div>
  );
}
