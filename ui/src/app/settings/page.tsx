"use client";

import { useState, useEffect } from "react";
import {
  Database, BrainCircuit, Network, Cloud, ArrowRight,
  Layers, FolderOpen, FolderInput, Wrench, Check, LogIn, LogOut,
  KeyRound, Users, ShieldCheck, TerminalSquare,
} from "lucide-react";
import { EmbeddingSelector } from "@/components/ingestion/EmbeddingSelector";
import { LLMSelector } from "@/components/ingestion/LLMSelector";
import {
  getPreference, nativeAvailable, openCloudLogin, pickFolder,
  resetOnboarding, revealPath, setPreference,
} from "@/lib/native";
import { cn } from "@/lib/utils";
import { StatusBadge } from "@/components/ui/status-badge";
import { createClient } from "@/utils/supabase/client";
import { ApiKeysManager } from "@/app/cloud/settings/api-keys/ApiKeysManager";
import { ServiceAccountsManager } from "@/app/cloud/settings/api-keys/ServiceAccountsManager";
import { TeamManager } from "@/app/cloud/settings/team/TeamManager";
import { SecurityManager } from "@/app/cloud/settings/security/SecurityManager";
import { IpAllowlistManager } from "@/app/cloud/settings/security/IpAllowlistManager";
import { DeveloperManager } from "@/app/cloud/settings/developer/DeveloperManager";

/* ─── Constants ────────────────────────────────────────────────────────── */

const IS_SAAS = process.env.NEXT_PUBLIC_VALORI_FORCE_CLOUD === "1";

const ALL_NAV = [
  { id: "general",    label: "General",           Icon: FolderInput,    group: "local"   },
  { id: "embedding",  label: "Embedding Engine",  Icon: Database,       group: "local"   },
  { id: "llm",        label: "Reasoning LLM",     Icon: BrainCircuit,   group: "local"   },
  { id: "backend",    label: "Backend",            Icon: Network,        group: "local"   },
  { id: "reranker",   label: "Tier-2 Reranker",   Icon: Layers,         group: "local"   },
  { id: "objstore",   label: "Object Store",       Icon: Cloud,          group: "local"   },
  { id: "cloudsync",  label: "Account",            Icon: LogIn,          group: "always"  },
  { id: "apikeys",    label: "API Keys",           Icon: KeyRound,       group: "cloud"   },
  { id: "team",       label: "Team",               Icon: Users,          group: "cloud"   },
  { id: "security",   label: "Security",           Icon: ShieldCheck,    group: "cloud"   },
  { id: "sdk",        label: "SDK",                Icon: TerminalSquare, group: "cloud"   },
  { id: "developer",  label: "Developer",          Icon: Wrench,         group: "desktop" },
] as const;

type NavId = typeof ALL_NAV[number]["id"];

/* ─── Primitives ──────────────────────────────────────────────────────── */

function SectionTitle({ children }: { children: React.ReactNode }) {
  return (
    <h2 className="text-sm font-semibold text-foreground mb-4 flex items-center gap-2">
      {children}
    </h2>
  );
}

function Card({ children, className }: { children: React.ReactNode; className?: string }) {
  return (
    <div className={cn("rounded-xl border border-border bg-card overflow-hidden", className)}>
      {children}
    </div>
  );
}

function Row({
  label,
  description,
  children,
  border = true,
}: {
  label: string;
  description?: string;
  children?: React.ReactNode;
  border?: boolean;
}) {
  return (
    <div className={cn("flex items-center justify-between gap-6 px-5 py-4", border && "border-b border-border/50 last:border-0")}>
      <div className="min-w-0">
        <p className="text-sm font-medium text-foreground">{label}</p>
        {description && <p className="text-xs text-muted-foreground mt-0.5">{description}</p>}
      </div>
      {children && <div className="flex shrink-0 items-center gap-2">{children}</div>}
    </div>
  );
}

function Badge({ children, variant = "neutral" }: { children: React.ReactNode; variant?: "neutral" | "success" | "warning" }) {
  const toneMap: Record<string, Parameters<typeof StatusBadge>[0]["tone"]> = {
    success: "success",
    warning: "warning",
    neutral: "neutral",
  };
  const tone = toneMap[variant] ?? "neutral";
  return (
    <StatusBadge tone={tone}>
      {children}
    </StatusBadge>
  );
}

function Btn({
  onClick,
  disabled,
  children,
  variant = "default",
}: {
  onClick?: () => void;
  disabled?: boolean;
  children: React.ReactNode;
  variant?: "default" | "danger";
}) {
  return (
    <button
      onClick={onClick}
      disabled={disabled}
      className={cn(
        "rounded-lg border px-3 py-1.5 text-xs font-medium transition-colors disabled:opacity-40",
        variant === "danger"
          ? "border-red-500/30 bg-red-500/10 text-red-600 hover:bg-red-500/20 dark:text-red-400"
          : "border-border bg-background text-foreground hover:bg-accent",
      )}
    >
      {children}
    </button>
  );
}

function Warning({ children }: { children: React.ReactNode }) {
  return (
    <p className="flex items-start gap-2 text-[11px] text-amber-600 dark:text-amber-400 px-5 pb-4">
      <span className="shrink-0 mt-0.5">⚠</span>
      <span>{children}</span>
    </p>
  );
}

/* ─── FolderRow ───────────────────────────────────────────────────────── */

function FolderRow({ label, path, onChange }: {
  label: string;
  path: string | null;
  onChange?: (path: string) => void;
}) {
  const canReveal = nativeAvailable() && !!path;
  return (
    <div className="flex items-center justify-between gap-4 px-5 py-3.5 border-b border-border/50 last:border-0">
      <div className="min-w-0">
        <p className="text-xs font-medium text-foreground whitespace-nowrap">{label}</p>
        <p className="mt-0.5 truncate font-mono text-[11px] text-muted-foreground" title={path ?? undefined}>
          {path ?? <span className="italic">Not set</span>}
        </p>
      </div>
      <div className="flex shrink-0 gap-1.5">
        {onChange && (
          <Btn onClick={async () => { const dir = await pickFolder(label); if (dir) onChange(dir); }}>
            {path ? "Change" : "Choose…"}
          </Btn>
        )}
        <button
          onClick={() => path && revealPath(path)}
          disabled={!canReveal}
          title={nativeAvailable() ? undefined : "Only available in the desktop app"}
          className="flex items-center gap-1.5 rounded-lg border border-border bg-background px-3 py-1.5 text-xs font-medium text-foreground hover:bg-accent disabled:opacity-40 transition-colors"
        >
          <FolderOpen size={12} />
          Open
        </button>
      </div>
    </div>
  );
}

/* ─── TestResult ──────────────────────────────────────────────────────── */

function TestResult({ ok, msg }: { ok: boolean; msg: string }) {
  return (
    <div className={cn(
      "flex items-center gap-2 text-xs px-5 pb-4 font-mono",
      ok ? "text-emerald-600 dark:text-emerald-400" : "text-red-600 dark:text-red-400",
    )}>
      {ok ? <Check size={12} /> : <span>✗</span>}
      {msg}
    </div>
  );
}

const RERANKER_STORAGE_KEY = "valori:reranker_config";

/* ─── Page ────────────────────────────────────────────────────────────── */

export default function SettingsPage() {
  const [testResult, setTestResult] = useState<{ ok: boolean; msg: string } | null>(null);
  const [testing, setTesting] = useState(false);
  const [serverPaths, setServerPaths]   = useState<{ event_log_path?: string; snapshot_path?: string; dim?: number } | null>(null);
  const [serverConfig, setServerConfig] = useState<{ api_url: string; auth_configured: boolean; object_store_configured: boolean; cluster_mode: boolean } | null>(null);
  const [rerankerProvider, setRerankerProvider] = useState<"none" | "cohere" | "custom">("none");
  const [rerankerApiKey,   setRerankerApiKey]   = useState("");
  const [rerankerModel,    setRerankerModel]     = useState("rerank-english-v3.0");
  const [rerankerEndpoint, setRerankerEndpoint] = useState("");
  const [configLoadFailed, setConfigLoadFailed] = useState(false);
  const [workspaceDir, setWorkspaceDir] = useState<string | null>(null);
  const [modelDir,     setModelDir]     = useState<string | null>(null);
  const [activeSection, setActiveSection] = useState<NavId>(IS_SAAS ? "cloudsync" : "general");
  const [cloudEmail, setCloudEmail] = useState<string | null | undefined>(undefined);
  const [cloudMode, setCloudMode] = useState<"local" | "cloud" | null>(null);
  const [signingOut, setSigningOut] = useState(false);

  // Cloud org data
  const [org, setOrg] = useState<{ id: string; name: string } | null>(null);
  const [myUserId, setMyUserId] = useState("");
  const [myRole, setMyRole] = useState("");
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  const [apiKeys, setApiKeys]           = useState<any[]>([]);
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  const [serviceAccounts, setServiceAccounts] = useState<any[]>([]);
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  const [members, setMembers]           = useState<any[]>([]);
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  const [invitations, setInvitations]   = useState<any[]>([]);
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  const [mfaFactors, setMfaFactors]     = useState<any[]>([]);
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  const [sessions, setSessions]         = useState<any[]>([]);
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  const [loginHistory, setLoginHistory] = useState<any[]>([]);
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  const [ipRules, setIpRules]           = useState<any[]>([]);
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  const [tokens, setTokens]             = useState<any[]>([]);

  useEffect(() => {
    getPreference<string>("workspaceDir").then(setWorkspaceDir).catch(() => {});
    getPreference<string>("modelDir").then(setModelDir).catch(() => {});
  }, []);

  useEffect(() => {
    if (!process.env.NEXT_PUBLIC_SUPABASE_URL) { setCloudEmail(null); return; }
    createClient().auth.getSession().then(({ data: { session } }) => {
      setCloudEmail(session?.user.email ?? null);
    });
    fetch("/api/mode").then((r) => r.ok ? r.json() : null).then((d) => {
      if (d) setCloudMode(d.mode);
    }).catch(() => {});
  }, []);

  // Fetch cloud org data once we know the user is signed in
  useEffect(() => {
    if (!cloudEmail || !process.env.NEXT_PUBLIC_SUPABASE_URL) return;
    const supabase = createClient();
    (async () => {
      const { data: { user } } = await supabase.auth.getUser();
      if (!user) return;
      setMyUserId(user.id);

      const { data: memberships } = await supabase
        .from("org_members")
        .select("role, organizations(id, name)")
        .eq("user_id", user.id)
        .limit(1);
      const mem = (memberships?.[0] ?? null) as { role: string; organizations: { id: string; name: string } } | null;
      if (mem?.organizations) {
        setOrg(mem.organizations);
        setMyRole(mem.role);
        const oid = mem.organizations.id;
        const [{ data: keys }, { data: accs }, { data: mems }, { data: invs }, { data: rules }] = await Promise.all([
          supabase.from("api_keys_public").select("*").eq("org_id", oid).order("created_at", { ascending: false }),
          supabase.from("service_accounts").select("*").eq("org_id", oid).order("created_at", { ascending: false }),
          supabase.rpc("list_org_members", { p_org_id: oid }),
          supabase.from("org_invitations").select("*").eq("org_id", oid).is("accepted_at", null).is("revoked_at", null).order("created_at", { ascending: false }),
          supabase.from("ip_allowlist_rules").select("*").eq("org_id", oid).order("created_at", { ascending: false }),
        ]);
        setApiKeys(keys ?? []);
        setServiceAccounts(accs ?? []);
        setMembers(mems ?? []);
        setInvitations(invs ?? []);
        setIpRules(rules ?? []);
      }

      const [mfaResult, { data: sess }, { data: hist }, { data: toks }] = await Promise.all([
        supabase.auth.mfa.listFactors(),
        supabase.rpc("list_my_sessions"),
        supabase.from("login_history").select("*").eq("user_id", user.id).order("created_at", { ascending: false }).limit(20),
        supabase.from("personal_access_tokens_public").select("*").order("created_at", { ascending: false }),
      ]);
      setMfaFactors(mfaResult.data?.all ?? []);
      setSessions(sess ?? []);
      setLoginHistory(hist ?? []);
      setTokens(toks ?? []);
    })().catch(console.error);
  }, [cloudEmail]);

  const signOutOfSync = async () => {
    setSigningOut(true);
    try {
      if (process.env.NEXT_PUBLIC_SUPABASE_URL) {
        await createClient().auth.signOut();
      }
      await fetch("/api/mode", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ mode: "local" }),
      }).catch(() => {});
      window.location.reload();
    } finally {
      setSigningOut(false);
    }
  };

  useEffect(() => {
    if (IS_SAAS) return;
    fetch("/api/health").then(r => r.ok ? r.json() : null).then(d => {
      if (d) setServerPaths({ event_log_path: d.event_log_path, snapshot_path: d.snapshot_path, dim: d.dim });
      else setConfigLoadFailed(true);
    }).catch(() => setConfigLoadFailed(true));
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
    } catch { localStorage.removeItem(RERANKER_STORAGE_KEY); }
  }, []);

  /* active-section tracking via IntersectionObserver */
  useEffect(() => {
    const root = document.querySelector("main") ?? undefined;
    const obs = new IntersectionObserver(
      (entries) => {
        for (const e of entries) {
          if (e.isIntersecting) setActiveSection(e.target.id as NavId);
        }
      },
      { root, rootMargin: "-10% 0px -70% 0px", threshold: 0 },
    );
    document.querySelectorAll("section[id]").forEach((s) => obs.observe(s));
    return () => obs.disconnect();
  }, [cloudEmail]); // re-observe when cloud sections mount/unmount

  const saveReranker = (update: Partial<{ provider: string; apiKey: string; model: string; endpoint: string }>) => {
    const next = {
      provider: update.provider ?? rerankerProvider,
      apiKey:   update.apiKey   ?? rerankerApiKey,
      model:    update.model    ?? rerankerModel,
      endpoint: update.endpoint ?? rerankerEndpoint,
    };
    try { localStorage.setItem(RERANKER_STORAGE_KEY, JSON.stringify(next)); } catch {}
    if (update.provider  !== undefined) setRerankerProvider(update.provider as "none" | "cohere" | "custom");
    if (update.apiKey    !== undefined) setRerankerApiKey(update.apiKey);
    if (update.model     !== undefined) setRerankerModel(update.model);
    if (update.endpoint  !== undefined) setRerankerEndpoint(update.endpoint);
  };

  const testConnection = async () => {
    setTesting(true); setTestResult(null);
    try {
      const res  = await fetch("/api/health");
      const data = await res.json().catch(() => ({})) as { status?: string };
      setTestResult(res.ok && data.status === "ok"
        ? { ok: true,  msg: "Connected to Valori backend" }
        : { ok: false, msg: `Backend returned ${res.status}` });
    } catch { setTestResult({ ok: false, msg: "Backend unreachable" }); }
    finally  { setTesting(false); }
  };

  const canManage = myRole === "owner" || myRole === "admin";

  const visibleNav = ALL_NAV.filter(({ group }) => {
    if (group === "local"   && IS_SAAS)        return false;
    if (group === "cloud"   && !cloudEmail)     return false;
    if (group === "desktop" && !nativeAvailable()) return false;
    return true;
  });

  return (
    <div className="flex gap-10 w-full max-w-4xl pb-20">

      {/* ── Left sticky nav ── */}
      <nav className="sticky top-0 self-start w-44 shrink-0 pt-0.5" aria-label="Settings sections">
        <p className="text-[10px] font-semibold uppercase tracking-widest text-muted-foreground px-2 mb-2">
          Settings
        </p>
        <ul className="flex flex-col gap-0.5">
          {visibleNav.map(({ id, label, Icon }) => (
            <li key={id}>
              <a
                href={`#${id}`}
                className={cn(
                  "flex items-center gap-2.5 rounded-lg px-2.5 py-2 text-sm font-medium transition-colors",
                  activeSection === id
                    ? "bg-[var(--v-accent-muted)] text-foreground [box-shadow:inset_2px_0_0_var(--v-accent)]"
                    : "text-muted-foreground hover:bg-accent/60 hover:text-foreground",
                )}
              >
                <Icon
                  size={14}
                  className={activeSection === id ? "text-[var(--v-accent)]" : "text-muted-foreground"}
                  aria-hidden
                />
                {label}
              </a>
            </li>
          ))}
        </ul>
      </nav>

      {/* ── Right scrolling content ── */}
      <div className="flex-1 min-w-0 flex flex-col gap-10">

        {!IS_SAAS && <>
          {/* General */}
          <section id="general" className="scroll-mt-4">
            <SectionTitle>General</SectionTitle>
            {!nativeAvailable() && (
              <p className="text-xs text-amber-600 dark:text-amber-400 mb-3 px-1">
                Folder management is only available in the Valori desktop app.
              </p>
            )}
            <Card>
              <FolderRow label="Workspace folder" path={workspaceDir} onChange={(d) => { setWorkspaceDir(d); setPreference("workspaceDir", d).catch(() => {}); }} />
              <FolderRow label="Model folder"     path={modelDir}     onChange={(d) => { setModelDir(d);     setPreference("modelDir", d).catch(() => {}); }} />
              <FolderRow label="Event log"        path={serverPaths?.event_log_path ?? null} />
              <FolderRow label="Snapshot"         path={serverPaths?.snapshot_path  ?? null} />
            </Card>
          </section>

          {/* Embedding Engine */}
          <section id="embedding" className="scroll-mt-4">
            <SectionTitle>Embedding Engine</SectionTitle>
            <Card className="p-6">
              <EmbeddingSelector />
            </Card>
          </section>

          {/* Reasoning LLM */}
          <section id="llm" className="scroll-mt-4">
            <SectionTitle>Reasoning LLM</SectionTitle>
            <Card className="p-6">
              <LLMSelector />
            </Card>
          </section>

          {/* Backend Connection */}
          <section id="backend" className="scroll-mt-4">
            <SectionTitle>Backend Connection</SectionTitle>
            <Card>
              <Row
                label="API routing"
                description="Requests to /api/* are proxied to the Valori Kernel."
              >
                <div className="flex items-center gap-2 flex-wrap justify-end">
                  <Badge>
                    {typeof window !== "undefined" ? window.location.origin : ""}/api/*
                  </Badge>
                  <ArrowRight size={11} className="text-muted-foreground" />
                  <Badge>{serverConfig?.api_url ?? "…"}</Badge>
                  {serverConfig?.cluster_mode && (
                    <Badge variant="success">cluster</Badge>
                  )}
                </div>
              </Row>

              {serverConfig && (
                <Row label="Authentication">
                  <div className="flex items-center gap-2 flex-wrap justify-end">
                    <Badge variant={serverConfig.auth_configured ? "success" : "neutral"}>
                      {serverConfig.auth_configured ? "✓ token set" : "open access"}
                    </Badge>
                    {serverConfig.object_store_configured && (
                      <Badge variant="success">✓ object store</Badge>
                    )}
                  </div>
                </Row>
              )}

              {serverPaths && (
                <Row label="Server configuration">
                  <div className="text-right flex flex-col gap-1">
                    <span className="font-mono text-xs text-muted-foreground">
                      <span className="text-foreground/60">DIM</span>{"  "}{serverPaths.dim ?? "—"}
                    </span>
                    {serverPaths.event_log_path && (
                      <span className="font-mono text-[11px] text-emerald-600 dark:text-emerald-400 truncate max-w-56" title={serverPaths.event_log_path}>
                        {serverPaths.event_log_path}
                      </span>
                    )}
                  </div>
                </Row>
              )}

              {testResult && <TestResult ok={testResult.ok} msg={testResult.msg} />}

              {serverConfig && !serverConfig.auth_configured && (
                <Warning>
                  Set <code className="font-mono">VALORI_AUTH_TOKEN</code> on the backend to require authentication.
                </Warning>
              )}
              {serverPaths && (!serverPaths.event_log_path || !serverPaths.snapshot_path) && (
                <Warning>
                  Without an event log and snapshot path, data is lost on restart.
                  Set <code className="font-mono">VALORI_EVENT_LOG_PATH</code> and <code className="font-mono">VALORI_SNAPSHOT_PATH</code>.
                </Warning>
              )}
              {configLoadFailed && !serverConfig && (
                <Warning>Couldn&apos;t load server configuration — the backend may be unreachable.</Warning>
              )}

              <div className="px-5 py-3 flex justify-end border-t border-border/50">
                <Btn onClick={testConnection} disabled={testing}>
                  {testing ? "Testing…" : "Test connection"}
                </Btn>
              </div>
            </Card>
          </section>

          {/* Tier-2 Reranker */}
          <section id="reranker" className="scroll-mt-4">
            <SectionTitle>Tier-2 Reranker</SectionTitle>
            <Card>
              <Row label="Provider" description="Cross-encoder applied after vector search. Non-determinism is logged in the proof receipt.">
                <div className="flex items-center gap-1.5">
                  {(["none", "cohere", "custom"] as const).map((p) => (
                    <button
                      key={p}
                      onClick={() => saveReranker({ provider: p })}
                      className={cn(
                        "px-3 py-1.5 text-xs rounded-lg border transition-colors",
                        rerankerProvider === p
                          ? "border-[var(--v-accent)] bg-[var(--v-accent-muted)] text-[var(--v-accent)]"
                          : "border-border bg-background text-muted-foreground hover:text-foreground hover:bg-accent",
                      )}
                    >
                      {p === "none" ? "Disabled" : p === "cohere" ? "Cohere" : "Custom"}
                    </button>
                  ))}
                </div>
              </Row>

              {rerankerProvider === "cohere" && (
                <>
                  <div className="px-5 py-4 border-t border-border/50 flex flex-col gap-3">
                    <div className="flex flex-col gap-1.5">
                      <label className="text-xs font-medium text-muted-foreground">API key</label>
                      <input
                        type="password"
                        value={rerankerApiKey}
                        onChange={(e) => saveReranker({ apiKey: e.target.value })}
                        placeholder="co-…"
                        className="rounded-lg border border-input bg-background px-3 py-2 text-sm text-foreground placeholder:text-muted-foreground focus:outline-none focus:ring-1 focus:ring-[var(--v-accent-ring)]"
                      />
                    </div>
                    <div className="flex flex-col gap-1.5">
                      <label className="text-xs font-medium text-muted-foreground">Model</label>
                      <select
                        value={rerankerModel}
                        onChange={(e) => saveReranker({ model: e.target.value })}
                        className="rounded-lg border border-input bg-background px-3 py-2 text-sm text-foreground focus:outline-none focus:ring-1 focus:ring-[var(--v-accent-ring)]"
                      >
                        <option>rerank-english-v3.0</option>
                        <option>rerank-multilingual-v3.0</option>
                        <option>rerank-english-v2.0</option>
                      </select>
                    </div>
                  </div>
                </>
              )}

              {rerankerProvider === "custom" && (
                <div className="px-5 py-4 border-t border-border/50 flex flex-col gap-1.5">
                  <label className="text-xs font-medium text-muted-foreground">Endpoint URL</label>
                  <input
                    type="text"
                    value={rerankerEndpoint}
                    onChange={(e) => saveReranker({ endpoint: e.target.value })}
                    placeholder="http://localhost:8080/rerank"
                    className="rounded-lg border border-input bg-background px-3 py-2 text-sm text-foreground placeholder:text-muted-foreground focus:outline-none focus:ring-1 focus:ring-[var(--v-accent-ring)]"
                  />
                  <p className="text-[11px] text-muted-foreground mt-1">
                    Must accept <code className="font-mono">{"{ query, documents }"}</code> → return <code className="font-mono">{"{ scores: number[] }"}</code>
                  </p>
                </div>
              )}
            </Card>
          </section>

          {/* Object Store */}
          <section id="objstore" className="scroll-mt-4">
            <SectionTitle>Object Store</SectionTitle>
            <Card>
              <Row label="S3 / MinIO / R2 backup" description="Configure via environment variables on the Valori Kernel process.">
                <a href="/snapshots" className="flex items-center gap-1.5 text-xs font-medium text-[var(--v-accent)] hover:underline">
                  Browse snapshots <ArrowRight size={11} />
                </a>
              </Row>
              <div className="px-5 py-4 border-t border-border/50">
                <div className="rounded-lg bg-accent/60 border border-border/60 px-4 py-3 font-mono text-xs text-muted-foreground space-y-1.5">
                  <p><span className="text-[var(--v-accent)]">VALORI_OBJECT_STORE_URL</span>=s3://my-bucket/valori</p>
                  <p><span className="text-[var(--v-accent)]">VALORI_OBJECT_STORE_REGION</span>=us-east-1</p>
                  <p><span className="text-[var(--v-accent)]">VALORI_OBJECT_STORE_KEEP</span>=7</p>
                </div>
              </div>
            </Card>
          </section>
        </>}

        {/* Account (always shown) */}
        <section id="cloudsync" className="scroll-mt-4">
          <SectionTitle>Account</SectionTitle>
          <Card>
            <Row
              label={cloudEmail ? "Signed in" : "Not signed in"}
              description={
                cloudEmail
                  ? `${cloudEmail} — this app is in ${cloudMode === "cloud" ? "cloud" : "local"} mode.`
                  : "Sign in to create and manage projects hosted on Valori Cloud."
              }
            >
              {cloudEmail === undefined ? null : cloudEmail ? (
                <Btn variant="danger" onClick={signOutOfSync} disabled={signingOut}>
                  <span className="flex items-center gap-1.5">
                    <LogOut size={12} />
                    {signingOut ? "Signing out…" : "Sign out"}
                  </span>
                </Btn>
              ) : (
                <Btn onClick={() => openCloudLogin()} disabled={!nativeAvailable()}>
                  <span className="flex items-center gap-1.5">
                    <LogIn size={12} />
                    Sign in to sync
                  </span>
                </Btn>
              )}
            </Row>
          </Card>
        </section>

        {/* Cloud sections — only when signed in */}
        {cloudEmail && <>
          {/* API Keys */}
          <section id="apikeys" className="scroll-mt-4">
            <SectionTitle>API Keys</SectionTitle>
            {org ? (
              <div className="flex flex-col gap-6">
                <ServiceAccountsManager
                  orgId={org.id}
                  canManage={canManage}
                  // eslint-disable-next-line @typescript-eslint/no-explicit-any
                  initialAccounts={serviceAccounts as any}
                />
                <ApiKeysManager
                  orgId={org.id}
                  canManage={canManage}
                  // eslint-disable-next-line @typescript-eslint/no-explicit-any
                  initialKeys={apiKeys as any}
                  // eslint-disable-next-line @typescript-eslint/no-explicit-any
                  serviceAccounts={serviceAccounts.filter((a: any) => !a.disabled_at) as any}
                />
              </div>
            ) : (
              <p className="text-sm text-muted-foreground">No organisation found for this account.</p>
            )}
          </section>

          {/* Team */}
          <section id="team" className="scroll-mt-4">
            <SectionTitle>Team</SectionTitle>
            {org ? (
              <TeamManager
                orgId={org.id}
                orgName={org.name}
                myUserId={myUserId}
                myRole={myRole}
                canManage={canManage}
                canChangeRoles={myRole === "owner"}
                // eslint-disable-next-line @typescript-eslint/no-explicit-any
                initialMembers={members as any}
                // eslint-disable-next-line @typescript-eslint/no-explicit-any
                initialInvitations={invitations as any}
              />
            ) : (
              <p className="text-sm text-muted-foreground">No organisation found for this account.</p>
            )}
          </section>

          {/* Security */}
          <section id="security" className="scroll-mt-4">
            <SectionTitle>Security</SectionTitle>
            <div className="flex flex-col gap-6">
              <SecurityManager
                // eslint-disable-next-line @typescript-eslint/no-explicit-any
                initialFactors={mfaFactors as any}
                // eslint-disable-next-line @typescript-eslint/no-explicit-any
                initialSessions={sessions as any}
                // eslint-disable-next-line @typescript-eslint/no-explicit-any
                initialLoginHistory={loginHistory as any}
              />
              {org && (
                <IpAllowlistManager
                  orgId={org.id}
                  orgName={org.name}
                  canManage={canManage}
                  // eslint-disable-next-line @typescript-eslint/no-explicit-any
                  initialRules={ipRules as any}
                />
              )}
            </div>
          </section>

          {/* SDK / Personal tokens */}
          <section id="sdk" className="scroll-mt-4">
            <SectionTitle>SDK &amp; Developer</SectionTitle>
            <DeveloperManager
              // eslint-disable-next-line @typescript-eslint/no-explicit-any
              initialTokens={tokens as any}
            />
          </section>
        </>}

        {/* Developer (desktop only, local mode only) */}
        {!IS_SAAS && nativeAvailable() && (
          <section id="developer" className="scroll-mt-4">
            <SectionTitle>Developer</SectionTitle>
            <Card>
              <Row
                label="Reset first-run experience"
                description="Shows the Welcome flow again on next launch without deleting any project data."
              >
                <Btn
                  variant="danger"
                  onClick={async () => { await resetOnboarding(); window.location.reload(); }}
                >
                  Reset onboarding
                </Btn>
              </Row>
            </Card>
          </section>
        )}

      </div>
    </div>
  );
}
