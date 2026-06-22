"use client";

import { use, useState, useEffect } from "react";
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
      <div className="flex items-center gap-2 text-sm text-muted-foreground">
        <Link href="/projects" className="hover:text-accent-foreground transition-colors">
          Projects
        </Link>
        <span>/</span>
        <span className="text-foreground font-medium">{project}</span>
      </div>

      <Tabs defaultValue="collections">
        <TabsList className="bg-card border border-border">
          <TabsTrigger
            value="collections"
            className="data-[state=active]:bg-muted data-[state=active]:text-foreground text-muted-foreground"
          >
            Collections
          </TabsTrigger>
          <TabsTrigger
            value="metrics"
            className="data-[state=active]:bg-muted data-[state=active]:text-foreground text-muted-foreground"
          >
            Metrics
          </TabsTrigger>
          <TabsTrigger
            value="settings"
            className="data-[state=active]:bg-muted data-[state=active]:text-foreground text-muted-foreground"
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
        <h3 className="text-xs font-medium text-muted-foreground uppercase tracking-widest mb-3">
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
        <h3 className="text-xs font-medium text-muted-foreground uppercase tracking-widest mb-3">
          Node health
        </h3>
        <div className="grid grid-cols-2 gap-4">
          <div className="rounded-xl border border-border bg-card p-4">
            <p className="text-[10px] uppercase tracking-widest text-muted-foreground">Status</p>
            <div className="mt-2 flex items-center gap-2">
              <span
                className={`h-2 w-2 rounded-full ${online ? "bg-emerald-400" : "bg-red-400 animate-pulse"}`}
              />
              <span className={`text-sm font-medium ${online ? "text-emerald-400" : "text-red-400"}`}>
                {online ? "Online" : "Unreachable"}
              </span>
            </div>
          </div>
          <div className="rounded-xl border border-border bg-card p-4">
            <p className="text-[10px] uppercase tracking-widest text-muted-foreground">State Hash</p>
            <code className="mt-2 block text-xs font-mono text-muted-foreground truncate">
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
  const [serverConfig, setServerConfig] = useState<{ api_url: string; auth_configured: boolean } | null>(null);
  const [embedCfg, setEmbedCfg] = useState<{ provider: string; model: string } | null>(null);

  useEffect(() => {
    fetch("/api/config").then(r => r.ok ? r.json() : null).then(d => {
      if (d) setServerConfig(d);
    }).catch(() => {});
    try {
      const raw = localStorage.getItem("valori:embedding_config");
      if (raw) {
        const c = JSON.parse(raw);
        setEmbedCfg({ provider: c.provider ?? "—", model: c.model ?? "—" });
      }
    } catch {}
  }, []);

  return (
    <div className="flex flex-col gap-6 max-w-xl">
      <Section title="Connection">
        <SettingRow label="Backend URL" value={serverConfig?.api_url ?? "…"} mono copyable />
        <SettingRow
          label="Status"
          value={online ? "Connected" : "Unreachable"}
          highlight={online ? "green" : "red"}
        />
        <div className="px-4 py-3 flex items-center justify-between">
          <div>
            <p className="text-sm text-accent-foreground">Authentication</p>
            <p className="text-xs text-muted-foreground">
              {serverConfig
                ? serverConfig.auth_configured
                  ? "VALORI_AUTH_TOKEN is set on the server"
                  : "No auth token — open access"
                : "…"}
            </p>
          </div>
          <span className={`text-[10px] font-mono px-2 py-0.5 rounded-full border ${
            serverConfig?.auth_configured
              ? "border-emerald-800 bg-emerald-950/40 text-emerald-400"
              : "border-input bg-accent text-muted-foreground"
          }`}>
            {serverConfig?.auth_configured ? "secured" : "open"}
          </span>
        </div>
      </Section>

      <Section title="Index configuration">
        <SettingRow label="Dimension" value={dim != null ? String(dim) : "—"} />
        <SettingRow label="Namespace prefix" value={`${project}--*`} mono />
        <SettingRow label="Max namespaces" value="1 024" sub="MAX_NAMESPACES hard limit" />
      </Section>

      <Section title="Embedding (text search)">
        <div className="px-4 py-4 flex flex-col gap-3">
          <div className="flex items-center justify-between gap-3">
            <div>
              <p className="text-sm text-accent-foreground">Active embedding model</p>
              <p className="text-xs text-muted-foreground">
                Shared across all projects — configure in{" "}
                <Link href="/settings" className="text-accent-foreground hover:underline">
                  Settings
                </Link>
              </p>
            </div>
            {embedCfg && (
              <span className="text-xs font-mono px-2 py-0.5 rounded border border-input bg-accent text-muted-foreground">
                {embedCfg.provider}/{embedCfg.model}
              </span>
            )}
          </div>
          {!embedCfg && (
            <p className="text-xs text-amber-500">
              No embedding model configured.{" "}
              <Link href="/settings" className="text-accent-foreground hover:underline">
                Configure in Settings →
              </Link>
            </p>
          )}
        </div>
      </Section>
    </div>
  );
}

function Section({ title, children }: { title: string; children: React.ReactNode }) {
  return (
    <div className="flex flex-col gap-2">
      <h3 className="text-xs font-medium text-muted-foreground uppercase tracking-widest">{title}</h3>
      <div className="rounded-xl border border-border bg-card divide-y divide-border">
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
        <p className="text-sm text-accent-foreground">{label}</p>
        {sub && <p className="text-xs text-muted-foreground">{sub}</p>}
      </div>
      <div className="flex items-center gap-2">
        <span
          className={`text-sm truncate max-w-[200px] ${mono ? "font-mono" : ""} ${
            highlight === "green"
              ? "text-emerald-400"
              : highlight === "red"
              ? "text-red-400"
              : "text-muted-foreground"
          }`}
        >
          {value}
        </span>
        {copyable && (
          <button
            onClick={copy}
            className="text-[10px] text-muted-foreground hover:text-muted-foreground transition-colors"
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
    <div className="rounded-xl border border-border bg-card px-4 py-4">
      <p className="text-[10px] uppercase tracking-widest text-muted-foreground">{label}</p>
      <p className={`mt-1.5 font-mono text-xl font-semibold ${warn ? "text-amber-400" : "text-foreground"}`}>
        {value}
      </p>
      {sub && (
        <p className={`mt-0.5 text-xs ${warn ? "text-amber-600" : "text-muted-foreground"}`}>{sub}</p>
      )}
    </div>
  );
}
