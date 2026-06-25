"use client";

import { useState, useEffect } from "react";
import { Database, BrainCircuit, Network, Cloud, ArrowRight, Layers } from "lucide-react";
import { EmbeddingSelector } from "@/components/ingestion/EmbeddingSelector";
import { LLMSelector } from "@/components/ingestion/LLMSelector";

function Section({ title, description, icon: Icon, children }: { title: string; description?: string; icon: any; children: React.ReactNode }) {
  return (
    <div className="flex flex-col md:flex-row gap-6 items-start py-8 border-b border-border/50 last:border-0">
      <div className="w-full md:w-64 flex-shrink-0">
        <div className="flex items-center gap-2.5 mb-2">
          <div className="w-7 h-7 rounded-lg bg-accent flex items-center justify-center border border-border/50 text-muted-foreground">
            <Icon size={14} />
          </div>
          <h2 className="text-sm font-medium text-foreground">{title}</h2>
        </div>
        {description && <p className="text-xs text-muted-foreground leading-relaxed pl-9.5">{description}</p>}
      </div>
      <div className="flex-1 w-full rounded-xl border border-border/60 bg-card/40 p-6 shadow-sm">
        {children}
      </div>
    </div>
  );
}

function TestResult({ ok, msg }: { ok: boolean; msg: string }) {
  return (
    <div className={`flex items-center gap-2 text-xs mt-3 px-3 py-2 rounded-md font-mono ${ok ? "bg-emerald-500/10 text-emerald-600 dark:text-emerald-400" : "bg-destructive/10 text-destructive"}`}>
      <span className="flex-shrink-0">{ok ? "✓" : "✗"}</span>
      <span>{msg}</span>
    </div>
  );
}

const RERANKER_STORAGE_KEY = "valori:reranker_config";

export default function SettingsPage() {
  const [testResult, setTestResult] = useState<{ ok: boolean; msg: string } | null>(null);
  const [testing, setTesting] = useState(false);
  const [serverPaths, setServerPaths] = useState<{ event_log_path?: string; snapshot_path?: string; dim?: number } | null>(null);
  const [serverConfig, setServerConfig] = useState<{ api_url: string; auth_configured: boolean; object_store_configured: boolean; cluster_mode: boolean } | null>(null);
  const [rerankerProvider, setRerankerProvider] = useState<"none" | "cohere" | "custom">("none");
  const [rerankerApiKey, setRerankerApiKey] = useState("");
  const [rerankerModel, setRerankerModel] = useState("rerank-english-v3.0");
  const [rerankerEndpoint, setRerankerEndpoint] = useState("");

  useEffect(() => {
    fetch("/api/health").then(r => r.ok ? r.json() : null).then(d => {
      if (d) setServerPaths({ event_log_path: d.event_log_path, snapshot_path: d.snapshot_path, dim: d.dim });
    }).catch(() => {});
    fetch("/api/config").then(r => r.ok ? r.json() : null).then(d => {
      if (d) setServerConfig(d);
    }).catch(() => {});
  }, []);

  useEffect(() => {
    try {
      const raw = localStorage.getItem(RERANKER_STORAGE_KEY);
      if (raw) {
        const c = JSON.parse(raw);
        setRerankerProvider(c.provider ?? "none");
        setRerankerApiKey(c.apiKey ?? "");
        setRerankerModel(c.model ?? "rerank-english-v3.0");
        setRerankerEndpoint(c.endpoint ?? "");
      }
    } catch {}
  }, []);

  const saveReranker = (update: Partial<{ provider: string; apiKey: string; model: string; endpoint: string }>) => {
    const next = {
      provider: update.provider ?? rerankerProvider,
      apiKey: update.apiKey ?? rerankerApiKey,
      model: update.model ?? rerankerModel,
      endpoint: update.endpoint ?? rerankerEndpoint,
    };
    try { localStorage.setItem(RERANKER_STORAGE_KEY, JSON.stringify(next)); } catch {}
    if (update.provider !== undefined) setRerankerProvider(update.provider as "none" | "cohere" | "custom");
    if (update.apiKey !== undefined) setRerankerApiKey(update.apiKey);
    if (update.model !== undefined) setRerankerModel(update.model);
    if (update.endpoint !== undefined) setRerankerEndpoint(update.endpoint);
  };

  const testConnection = async () => {
    setTesting(true);
    setTestResult(null);
    try {
      const res = await fetch("/api/health");
      const data = await res.json().catch(() => ({})) as { status?: string };
      if (res.ok && data.status === "ok") {
        setTestResult({ ok: true, msg: "Connected to Valori backend" });
      } else {
        setTestResult({ ok: false, msg: `Backend returned ${res.status}` });
      }
    } catch {
      setTestResult({ ok: false, msg: "Backend unreachable" });
    } finally {
      setTesting(false);
    }
  };

  return (
    <div className="flex flex-col max-w-4xl pb-10">
      <div className="mb-6">
        <h1 className="text-2xl font-semibold text-foreground tracking-tight">Configuration</h1>
        <p className="mt-2 text-sm text-muted-foreground">
          Manage your embedding models, LLMs, and connection settings.
        </p>
      </div>

      <div className="flex flex-col">
        <Section title="Embedding Engine" description="Configure the embedding model used for vectorizing knowledge graph nodes and documents." icon={Database}>
          <EmbeddingSelector />
        </Section>

        <Section title="Reasoning LLM" description="Configure the LLM used for extraction, evaluation, and the 'Why this decision?' feature." icon={BrainCircuit}>
          <LLMSelector />
        </Section>

        <Section title="Backend Connection" description="Valori Kernel connection status and API routing." icon={Network}>
          <div className="flex flex-col gap-4">
            <div className="flex items-start justify-between gap-4">
              <div>
                <p className="text-sm font-medium text-foreground">Valori API</p>
                <div className="flex items-center gap-2 mt-1.5 flex-wrap">
                  <span className="rounded bg-accent px-2 py-0.5 text-xs font-mono text-muted-foreground border border-border/50">
                    {typeof window !== "undefined" ? window.location.origin : ""}/api/*
                  </span>
                  <ArrowRight size={12} className="text-muted-foreground" />
                  <span className="rounded bg-accent px-2 py-0.5 text-xs font-mono text-muted-foreground border border-border/50">
                    {serverConfig?.api_url ?? "…"}
                  </span>
                  {serverConfig?.cluster_mode && (
                    <span className="rounded bg-blue-500/15 border border-blue-500/30 px-2 py-0.5 text-[10px] text-blue-400">
                      cluster mode
                    </span>
                  )}
                </div>
              </div>
              <button
                onClick={testConnection}
                disabled={testing}
                className="flex-shrink-0 rounded-lg border border-border bg-background px-4 py-2 text-xs font-medium text-foreground hover:bg-accent hover:text-accent-foreground disabled:opacity-40 transition-colors shadow-sm"
              >
                {testing ? "Testing…" : "Test connection"}
              </button>
            </div>
            {testResult && <TestResult ok={testResult.ok} msg={testResult.msg} />}

            {/* Auth status */}
            {serverConfig && (
              <div className="rounded-lg border border-border/60 bg-background px-4 py-3">
                <p className="text-xs font-medium text-foreground mb-2">Authentication</p>
                <div className="flex items-center gap-3">
                  <span className={`text-[10px] font-mono px-2 py-0.5 rounded-full border ${
                    serverConfig.auth_configured
                      ? "border-emerald-800 bg-emerald-950/40 text-emerald-400"
                      : "border-input bg-accent text-muted-foreground"
                  }`}>
                    {serverConfig.auth_configured ? "✓ VALORI_AUTH_TOKEN set" : "no auth token (open access)"}
                  </span>
                  {serverConfig.object_store_configured && (
                    <span className="text-[10px] font-mono px-2 py-0.5 rounded-full border border-emerald-800 bg-emerald-950/40 text-emerald-400">
                      ✓ object store configured
                    </span>
                  )}
                </div>
                {!serverConfig.auth_configured && (
                  <p className="text-[11px] text-amber-500 mt-2">
                    ⚠ Set <code className="font-mono">VALORI_AUTH_TOKEN</code> on the backend to require authentication.
                  </p>
                )}
              </div>
            )}

            {/* Live server config paths — read from /health */}
            {serverPaths && (
              <div className="rounded-lg border border-border/60 bg-background px-4 py-3 space-y-2">
                <p className="text-xs font-medium text-foreground mb-2">Active server configuration</p>
                <div className="grid grid-cols-[auto_1fr] gap-x-3 gap-y-1.5 text-xs font-mono">
                  <span className="text-muted-foreground">DIM</span>
                  <span className="text-foreground">{serverPaths.dim ?? "—"}</span>

                  <span className="text-muted-foreground">EVENT LOG</span>
                  <span className={serverPaths.event_log_path ? "text-emerald-500" : "text-muted-foreground"}>
                    {serverPaths.event_log_path ?? "not configured (in-memory only)"}
                  </span>

                  <span className="text-muted-foreground">SNAPSHOT</span>
                  <span className={serverPaths.snapshot_path ? "text-emerald-500" : "text-muted-foreground"}>
                    {serverPaths.snapshot_path ?? "not configured"}
                  </span>
                </div>
                {(!serverPaths.event_log_path || !serverPaths.snapshot_path) && (
                  <p className="text-[11px] text-amber-500 mt-2">
                    ⚠ Without an event log and snapshot path, data is lost on restart.
                    Set <code className="font-mono">VALORI_EVENT_LOG_PATH</code> and <code className="font-mono">VALORI_SNAPSHOT_PATH</code>.
                  </p>
                )}
              </div>
            )}
          </div>
        </Section>

        <Section title="Tier-2 Reranker" description="Optional cross-encoder reranker applied after vector search. Scores are logged in the proof receipt so non-determinism is documented." icon={Layers}>
          <div className="flex flex-col gap-4">
            <div className="flex items-center gap-3">
              {(["none", "cohere", "custom"] as const).map((p) => (
                <button
                  key={p}
                  onClick={() => saveReranker({ provider: p })}
                  className={`px-3 py-1.5 text-xs rounded-lg border transition-colors capitalize ${
                    rerankerProvider === p
                      ? "border-primary bg-primary/10 text-primary"
                      : "border-border bg-background text-muted-foreground hover:text-foreground"
                  }`}
                >
                  {p === "none" ? "Disabled" : p === "cohere" ? "Cohere Rerank" : "Custom endpoint"}
                </button>
              ))}
            </div>
            {rerankerProvider === "cohere" && (
              <div className="flex flex-col gap-3">
                <div className="flex flex-col gap-1.5">
                  <label className="text-xs text-muted-foreground">API Key</label>
                  <input
                    type="password"
                    value={rerankerApiKey}
                    onChange={(e) => saveReranker({ apiKey: e.target.value })}
                    placeholder="co-..."
                    className="bg-accent border border-input text-accent-foreground text-sm rounded-lg px-3 py-2 focus:outline-none focus:border-ring"
                  />
                </div>
                <div className="flex flex-col gap-1.5">
                  <label className="text-xs text-muted-foreground">Model</label>
                  <select
                    value={rerankerModel}
                    onChange={(e) => saveReranker({ model: e.target.value })}
                    className="bg-accent border border-input text-accent-foreground text-sm rounded-lg px-3 py-2 focus:outline-none focus:border-ring"
                  >
                    <option>rerank-english-v3.0</option>
                    <option>rerank-multilingual-v3.0</option>
                    <option>rerank-english-v2.0</option>
                  </select>
                </div>
              </div>
            )}
            {rerankerProvider === "custom" && (
              <div className="flex flex-col gap-1.5">
                <label className="text-xs text-muted-foreground">Endpoint URL</label>
                <input
                  type="text"
                  value={rerankerEndpoint}
                  onChange={(e) => saveReranker({ endpoint: e.target.value })}
                  placeholder="http://localhost:8080/rerank"
                  className="bg-accent border border-input text-accent-foreground text-sm rounded-lg px-3 py-2 focus:outline-none focus:border-ring"
                />
                <p className="text-[11px] text-muted-foreground">Must accept <code className="font-mono">{"{ query, documents }"}</code> and return <code className="font-mono">{"{ scores: number[] }"}</code>.</p>
              </div>
            )}
            {rerankerProvider !== "none" && (
              <p className="text-[11px] text-muted-foreground">
                Reranking is non-deterministic. Scores appear in the proof receipt with a <code className="font-mono">reranked: true</code> flag so auditors know chunks were reordered.
              </p>
            )}
          </div>
        </Section>

        <Section title="Object Store" description="S3 / MinIO / R2 snapshot storage. Configured via environment variables on the server." icon={Cloud}>
          <div className="flex flex-col gap-4">
            <div className="rounded-lg bg-background border border-border px-4 py-3 font-mono text-xs text-muted-foreground space-y-1.5">
              <p><span className="text-[var(--v-accent)]">VALORI_OBJECT_STORE_URL</span>=s3://my-bucket/valori</p>
              <p><span className="text-[var(--v-accent)]">VALORI_OBJECT_STORE_REGION</span>=us-east-1</p>
              <p><span className="text-[var(--v-accent)]">VALORI_OBJECT_STORE_KEEP</span>=7</p>
            </div>
            <div>
              <a
                href="/settings/snapshots"
                className="inline-flex items-center gap-1.5 text-xs font-medium text-[var(--v-accent)] hover:underline"
              >
                Browse snapshots
                <ArrowRight size={12} />
              </a>
            </div>
          </div>
        </Section>
      </div>
    </div>
  );
}
