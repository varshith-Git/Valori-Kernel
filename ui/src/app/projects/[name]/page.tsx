"use client";

import { use, useState } from "react";
import Link from "next/link";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { CollectionList } from "@/components/collections/CollectionList";
import { useCollections } from "@/lib/hooks/useCollections";
import { useHealth } from "@/lib/hooks/useHealth";
import { useProof } from "@/lib/hooks/useProof";

export default function ProjectPage({
  params,
}: {
  params: Promise<{ name: string }>;
}) {
  const { name } = use(params);
  const project = decodeURIComponent(name);

  return (
    <div className="flex flex-col gap-6 max-w-5xl">
      {/* Breadcrumb */}
      <div className="flex items-center gap-2 text-sm text-zinc-500">
        <Link href="/projects" className="hover:text-zinc-300 transition-colors">
          Projects
        </Link>
        <span>/</span>
        <span className="text-white font-medium">{project}</span>
      </div>

      <Tabs defaultValue="collections">
        <TabsList className="bg-zinc-900 border border-zinc-800">
          <TabsTrigger
            value="collections"
            className="data-[state=active]:bg-zinc-700 data-[state=active]:text-white text-zinc-400"
          >
            Collections
          </TabsTrigger>
          <TabsTrigger
            value="metrics"
            className="data-[state=active]:bg-zinc-700 data-[state=active]:text-white text-zinc-400"
          >
            Metrics
          </TabsTrigger>
          <TabsTrigger
            value="settings"
            className="data-[state=active]:bg-zinc-700 data-[state=active]:text-white text-zinc-400"
          >
            Settings
          </TabsTrigger>
        </TabsList>

        <TabsContent value="collections" className="mt-5">
          <CollectionsTab project={project} />
        </TabsContent>
        <TabsContent value="metrics" className="mt-5">
          <MetricsTab project={project} />
        </TabsContent>
        <TabsContent value="settings" className="mt-5">
          <SettingsTab project={project} />
        </TabsContent>
      </Tabs>
    </div>
  );
}

function CollectionsTab({ project }: { project: string }) {
  const { collections, isLoading, create, drop } = useCollections(project);
  return (
    <CollectionList
      project={project}
      collections={collections}
      isLoading={isLoading}
      onCreate={create}
      onDrop={drop}
    />
  );
}

function MetricsTab({ project }: { project: string }) {
  const { collections } = useCollections(project);
  const { recordCount, chainHeight, dim, fillPct, online } = useHealth();
  const { hash } = useProof();

  const storageBytes =
    recordCount != null && dim != null ? recordCount * dim * 4 : null;
  const storageMB =
    storageBytes != null ? (storageBytes / 1024 / 1024).toFixed(2) : null;

  return (
    <div className="flex flex-col gap-6">
      <div className="grid grid-cols-4 gap-4">
        <MetricCard label="Collections" value={String(collections.length)} sub="in this project" />
        <MetricCard
          label="Est. Records"
          value={recordCount != null ? recordCount.toLocaleString() : "—"}
          sub="node total (approx.)"
        />
        <MetricCard
          label="Storage"
          value={storageMB != null ? `${storageMB} MB` : "—"}
          sub={dim != null ? `dim=${dim} × 4 B` : ""}
        />
        <MetricCard
          label="Fill"
          value={fillPct != null ? `${(fillPct * 100).toFixed(1)}%` : "—"}
          sub="pool utilisation"
          warn={fillPct != null && fillPct > 0.85}
        />
      </div>

      <div>
        <h3 className="text-xs font-medium text-zinc-500 uppercase tracking-widest mb-3">
          Request activity
        </h3>
        <div className="grid grid-cols-3 gap-4">
          <MetricCard
            label="Write requests"
            value={chainHeight != null ? chainHeight.toLocaleString() : "—"}
            sub="events in audit chain"
          />
          <MetricCard label="Read requests" value="—" sub="not tracked by backend" />
          <MetricCard label="Errors" value="—" sub="not tracked by backend" />
        </div>
      </div>

      <div>
        <h3 className="text-xs font-medium text-zinc-500 uppercase tracking-widest mb-3">
          Node health
        </h3>
        <div className="grid grid-cols-2 gap-4">
          <div className="rounded-xl border border-zinc-800 bg-zinc-900 p-4">
            <p className="text-[10px] uppercase tracking-widest text-zinc-600">Status</p>
            <div className="mt-2 flex items-center gap-2">
              <span
                className={`h-2 w-2 rounded-full ${online ? "bg-emerald-400" : "bg-red-400 animate-pulse"}`}
              />
              <span className={`text-sm font-medium ${online ? "text-emerald-400" : "text-red-400"}`}>
                {online ? "Online" : "Unreachable"}
              </span>
            </div>
          </div>
          <div className="rounded-xl border border-zinc-800 bg-zinc-900 p-4">
            <p className="text-[10px] uppercase tracking-widest text-zinc-600">State Hash</p>
            <code className="mt-2 block text-xs font-mono text-zinc-400 truncate">
              {hash ? `${hash.slice(0, 32)}…` : "—"}
            </code>
          </div>
        </div>
      </div>
    </div>
  );
}

function SettingsTab({ project }: { project: string }) {
  const { dim, online } = useHealth();
  const [tokenVisible, setTokenVisible] = useState(false);

  return (
    <div className="flex flex-col gap-6 max-w-xl">
      <Section title="Connection">
        <SettingRow label="Backend URL" value="http://localhost:3000" mono copyable />
        <SettingRow
          label="Status"
          value={online ? "Connected" : "Unreachable"}
          highlight={online ? "green" : "red"}
        />
      </Section>

      <Section title="Token / Auth">
        <div className="px-4 py-3 flex flex-col gap-2">
          <div className="flex items-center justify-between">
            <div>
              <p className="text-sm text-zinc-300">Bearer token</p>
              <p className="text-xs text-zinc-600">
                Set via <code className="font-mono">VALORI_AUTH_TOKEN</code> env var on the backend
              </p>
            </div>
            <button
              onClick={() => setTokenVisible((v) => !v)}
              className="text-xs text-zinc-500 hover:text-zinc-300 transition-colors"
            >
              {tokenVisible ? "Hide" : "Show"}
            </button>
          </div>
          {tokenVisible && (
            <div className="rounded-lg bg-zinc-950 border border-zinc-800 px-3 py-2">
              <code className="text-xs font-mono text-zinc-400">
                (configured server-side only — not exposed to browser)
              </code>
            </div>
          )}
        </div>
      </Section>

      <Section title="Index configuration">
        <SettingRow label="Dimension" value={dim != null ? String(dim) : "—"} />
        <SettingRow label="Namespace prefix" value={`${project}--*`} mono />
        <SettingRow label="Max namespaces" value="1 024" sub="MAX_NAMESPACES hard limit" />
      </Section>

      <Section title="Embedding (text search)">
        <div className="px-4 py-4 flex flex-col gap-3">
          <div className="flex items-center gap-2">
            <p className="text-sm text-zinc-300">OpenAI-compatible endpoint</p>
            <span className="text-[10px] rounded px-1.5 py-0.5 bg-zinc-800 text-zinc-500 border border-zinc-700">
              coming soon
            </span>
          </div>
          <p className="text-xs text-zinc-600">
            Configure an embedding API to enable Text and Hybrid search modes. Pre-computed
            vectors can be used today via Semantic mode.
          </p>
          <div className="flex flex-col gap-2 opacity-50 pointer-events-none">
            <input
              disabled
              placeholder="https://api.openai.com/v1/embeddings"
              className="w-full rounded-lg bg-zinc-800 border border-zinc-700 text-zinc-400 text-xs px-3 py-2"
            />
            <input
              disabled
              placeholder="sk-… (API key)"
              className="w-full rounded-lg bg-zinc-800 border border-zinc-700 text-zinc-400 text-xs px-3 py-2"
            />
          </div>
        </div>
      </Section>
    </div>
  );
}

function Section({ title, children }: { title: string; children: React.ReactNode }) {
  return (
    <div className="flex flex-col gap-2">
      <h3 className="text-xs font-medium text-zinc-500 uppercase tracking-widest">{title}</h3>
      <div className="rounded-xl border border-zinc-800 bg-zinc-900 divide-y divide-zinc-800">
        {children}
      </div>
    </div>
  );
}

function SettingRow({
  label,
  value,
  sub,
  mono,
  copyable,
  highlight,
}: {
  label: string;
  value: string;
  sub?: string;
  mono?: boolean;
  copyable?: boolean;
  highlight?: "green" | "red";
}) {
  const [copied, setCopied] = useState(false);
  const copy = () => {
    navigator.clipboard.writeText(value);
    setCopied(true);
    setTimeout(() => setCopied(false), 1500);
  };

  return (
    <div className="flex items-center justify-between px-4 py-3">
      <div>
        <p className="text-sm text-zinc-300">{label}</p>
        {sub && <p className="text-xs text-zinc-600">{sub}</p>}
      </div>
      <div className="flex items-center gap-2">
        <span
          className={`text-sm truncate max-w-[200px] ${mono ? "font-mono" : ""} ${
            highlight === "green"
              ? "text-emerald-400"
              : highlight === "red"
              ? "text-red-400"
              : "text-zinc-400"
          }`}
        >
          {value}
        </span>
        {copyable && (
          <button
            onClick={copy}
            className="text-[10px] text-zinc-600 hover:text-zinc-400 transition-colors"
          >
            {copied ? "✓" : "copy"}
          </button>
        )}
      </div>
    </div>
  );
}

function MetricCard({
  label,
  value,
  sub,
  warn,
}: {
  label: string;
  value: string;
  sub?: string;
  warn?: boolean;
}) {
  return (
    <div className="rounded-xl border border-zinc-800 bg-zinc-900 px-4 py-4">
      <p className="text-[10px] uppercase tracking-widest text-zinc-600">{label}</p>
      <p className={`mt-1.5 font-mono text-xl font-semibold ${warn ? "text-amber-400" : "text-white"}`}>
        {value}
      </p>
      {sub && (
        <p className={`mt-0.5 text-xs ${warn ? "text-amber-600" : "text-zinc-600"}`}>{sub}</p>
      )}
    </div>
  );
}
