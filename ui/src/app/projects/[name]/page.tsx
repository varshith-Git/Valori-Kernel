"use client";

import { use, useState } from "react";
import Link from "next/link";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { RecordsTab } from "@/components/projects/RecordsTab";
import { UploadTab } from "@/components/projects/UploadTab";
import { SearchTab } from "@/components/projects/SearchTab";
import { useProjects } from "@/lib/hooks/useProjects";
import { DeleteProjectDialog } from "@/components/projects/DeleteProjectDialog";
import { useRouter } from "next/navigation";

export default function ProjectDetailPage({
  params,
}: {
  params: Promise<{ name: string }>;
}) {
  const { name } = use(params);
  const decoded = decodeURIComponent(name);
  const router = useRouter();
  const { drop } = useProjects();
  const [deleteOpen, setDeleteOpen] = useState(false);

  return (
    <>
      <div className="flex flex-col gap-6 max-w-4xl">
        {/* Breadcrumb */}
        <div className="flex items-center gap-2 text-sm">
          <Link href="/projects" className="text-zinc-500 hover:text-zinc-300 transition-colors">
            Projects
          </Link>
          <span className="text-zinc-700">/</span>
          <span className="text-white font-medium">{decoded}</span>
          <button
            onClick={() => setDeleteOpen(true)}
            className="ml-auto text-xs text-zinc-600 hover:text-red-400 transition-colors border border-zinc-800 rounded px-2 py-1"
          >
            Delete project
          </button>
        </div>

        {/* Tabs */}
        <Tabs defaultValue="records">
          <TabsList className="bg-zinc-900 border border-zinc-800">
            <TabsTrigger
              value="records"
              className="text-zinc-400 data-[state=active]:text-white data-[state=active]:bg-zinc-800"
            >
              Records
            </TabsTrigger>
            <TabsTrigger
              value="upload"
              className="text-zinc-400 data-[state=active]:text-white data-[state=active]:bg-zinc-800"
            >
              Upload
            </TabsTrigger>
            <TabsTrigger
              value="search"
              className="text-zinc-400 data-[state=active]:text-white data-[state=active]:bg-zinc-800"
            >
              Search
            </TabsTrigger>
          </TabsList>

          <TabsContent value="records" className="mt-4">
            <RecordsTab collection={decoded} />
          </TabsContent>
          <TabsContent value="upload" className="mt-4">
            <UploadTab collection={decoded} />
          </TabsContent>
          <TabsContent value="search" className="mt-4">
            <SearchTab collection={decoded} />
          </TabsContent>
        </Tabs>
      </div>

      <DeleteProjectDialog
        name={decoded}
        open={deleteOpen}
        onClose={() => setDeleteOpen(false)}
        onDelete={async () => {
          await drop(decoded);
          router.push("/projects");
        }}
      />
    </>
  );
}
