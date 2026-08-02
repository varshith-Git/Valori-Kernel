"use client";

import { useEffect, useRef, useState, useCallback } from "react";
import {
  Search, X, SlidersHorizontal, UserCircle, Lock, CreditCard,
  BarChart2, Info, FolderOpen, ChevronRight, Check, Trash2,
  LogOut, ShieldCheck, ExternalLink, Monitor,
  Sun, Moon, Bell, Database, BrainCircuit, Network,
  Layers, Cloud, Wrench, ArrowRight, AlertTriangle,
} from "lucide-react";
import { cn } from "@/lib/utils";
import { useTheme, type ThemePref } from "@/lib/theme";
import { useHealth } from "@/lib/hooks/useHealth";
import {
  nativeAvailable, pickFolder, revealPath,
  getPreference, setPreference, resetOnboarding,
} from "@/lib/native";
import { EmbeddingSelector } from "@/components/ingestion/EmbeddingSelector";
import { LLMSelector } from "@/components/ingestion/LLMSelector";
import { createClient } from "@/utils/supabase/client";

/* ─── Types ────────────────────────────────────────────────────────────── */

type SectionId = "general" | "account" | "privacy" | "billing" | "usage" | "about";

interface UserData {
  id: string;
  email: string;
  firstName: string;
  lastName: string;
  avatarUrl?: string;
  provider?: string;
  createdAt?: string;
  orgId?: string;
  orgName?: string;
  role?: string;
}

interface SessionRow {
  session_id: string;
  created_at: string;
  updated_at: string;
  user_agent: string | null;
  ip: string | null;
  is_current: boolean;
}

/* ─── Nav ──────────────────────────────────────────────────────────────── */

const NAV: { id: SectionId; label: string; Icon: React.ElementType }[] = [
  { id: "general",  label: "General",  Icon: SlidersHorizontal },
  { id: "account",  label: "Account",  Icon: UserCircle },
  { id: "privacy",  label: "Privacy",  Icon: Lock },
  { id: "billing",  label: "Billing",  Icon: CreditCard },
  { id: "usage",    label: "Usage",    Icon: BarChart2 },
  { id: "about",    label: "About",    Icon: Info },
];

/* ─── Shared primitives ────────────────────────────────────────────────── */

function SCard({ children, className }: { children: React.ReactNode; className?: string }) {
  return (
    <div className={cn("rounded-xl border border-border bg-card overflow-hidden", className)}>
      {children}
    </div>
  );
}

function SCardHeader({ title, description }: { title: string; description?: string }) {
  return (
    <div className="px-5 pt-5 pb-4 border-b border-border/60">
      <h3 className="text-sm font-semibold text-foreground">{title}</h3>
      {description && <p className="text-xs text-muted-foreground mt-0.5">{description}</p>}
    </div>
  );
}

function SRow({
  label,
  description,
  children,
  last,
}: {
  label: string;
  description?: string;
  children?: React.ReactNode;
  last?: boolean;
}) {
  return (
    <div className={cn("flex items-center justify-between gap-6 px-5 py-3.5", !last && "border-b border-border/40")}>
      <div className="min-w-0">
        <p className="text-sm text-foreground">{label}</p>
        {description && <p className="text-[11px] text-muted-foreground mt-0.5">{description}</p>}
      </div>
      {children && <div className="shrink-0">{children}</div>}
    </div>
  );
}

function SBtn({
  onClick,
  disabled,
  disabledReason,
  children,
  variant = "default",
  className,
}: {
  onClick?: () => void;
  disabled?: boolean;
  disabledReason?: string;
  children: React.ReactNode;
  variant?: "default" | "danger" | "primary";
  className?: string;
}) {
  return (
    <button
      onClick={onClick}
      disabled={disabled}
      title={disabled ? disabledReason : undefined}
      className={cn(
        "rounded-lg px-3.5 py-1.5 text-xs font-medium border transition-colors",
        "disabled:opacity-45 disabled:cursor-not-allowed disabled:hover:bg-transparent",
        !disabled && "cursor-pointer active:scale-[0.97]",
        variant === "danger"
          ? "border-red-500/30 bg-red-500/10 text-red-600 hover:bg-red-500/20 dark:text-red-400"
          : variant === "primary"
          ? "border-[var(--v-accent)] bg-[var(--v-accent)] text-white hover:opacity-90 shadow-sm"
          : "border-border bg-background text-foreground hover:bg-accent hover:border-border/80",
        className,
      )}
    >
      {children}
    </button>
  );
}

function ComingSoon() {
  return (
    <span className="text-[10px] font-medium text-muted-foreground bg-accent/70 border border-border/60 rounded-full px-2 py-1">
      Coming soon
    </span>
  );
}

function Toggle({ checked, onChange, disabled }: { checked: boolean; onChange: (v: boolean) => void; disabled?: boolean }) {
  return (
    <button
      type="button"
      role="switch"
      aria-checked={checked}
      disabled={disabled}
      onClick={() => onChange(!checked)}
      className={cn(
        "relative h-5 w-9 shrink-0 rounded-full transition-colors duration-200",
        "disabled:opacity-45 disabled:cursor-not-allowed",
        !disabled && "cursor-pointer",
        checked ? "bg-[var(--v-accent)]" : "bg-input hover:bg-muted-foreground/30",
      )}
    >
      <span
        className={cn(
          "absolute top-0.5 left-0.5 h-4 w-4 rounded-full bg-white shadow-sm ring-1 ring-black/5 transition-transform duration-200",
          checked ? "translate-x-4" : "translate-x-0",
        )}
      />
    </button>
  );
}

function DangerCard({ children }: { children: React.ReactNode }) {
  return (
    <div className="rounded-xl border border-red-500/25 bg-red-500/5 overflow-hidden">
      <div className="px-5 pt-4 pb-3 border-b border-red-500/20 flex items-center gap-2">
        <AlertTriangle size={13} className="text-red-500" />
        <h3 className="text-sm font-semibold text-red-600 dark:text-red-400">Danger Zone</h3>
      </div>
      {children}
    </div>
  );
}

/* ─── General section ───────────────────────────────────────────────────── */

function GeneralSection({ user, onUserUpdate }: { user: UserData | null; onUserUpdate: (u: Partial<UserData>) => void }) {
  const { pref, setTheme } = useTheme();
  const [firstName, setFirstName] = useState(user?.firstName ?? "");
  const [lastName,  setLastName]  = useState(user?.lastName ?? "");
  const [saving, setSaving] = useState(false);
  const [saved, setSaved]   = useState(false);
  const [workspaceDir, setWorkspaceDir] = useState<string | null>(null);
  const [serverConfig, setServerConfig] = useState<{ api_url?: string; dim?: number; event_log_path?: string } | null>(null);
  const [notifPrefs, setNotifPrefs] = useState(() => {
    try { return JSON.parse(localStorage.getItem("valori:notifs") ?? "{}"); } catch { return {}; }
  });
  const [rerankerProvider, setRerankerProvider] = useState<"none"|"cohere"|"custom">("none");
  const [rerankerKey, setRerankerKey] = useState("");
  const [rerankerModel, setRerankerModel] = useState("rerank-english-v3.0");
  const [rerankerEndpoint, setRerankerEndpoint] = useState("");

  useEffect(() => {
    setFirstName(user?.firstName ?? "");
    setLastName(user?.lastName ?? "");
  }, [user?.firstName, user?.lastName]);

  useEffect(() => {
    if (nativeAvailable()) getPreference<string>("workspaceDir").then(setWorkspaceDir).catch(() => {});
    fetch("/api/health").then(r => r.ok ? r.json() : null).then(d => {
      if (d) setServerConfig({ api_url: d.api_url, dim: d.dim, event_log_path: d.event_log_path });
    }).catch(() => {});
    try {
      const raw = localStorage.getItem("valori:reranker_config");
      if (raw) {
        const c = JSON.parse(raw);
        setRerankerProvider(c.provider ?? "none");
        setRerankerKey(c.apiKey ?? "");
        setRerankerModel(c.model ?? "rerank-english-v3.0");
        setRerankerEndpoint(c.endpoint ?? "");
      }
    } catch {}
  }, []);

  const saveProfile = async () => {
    if (!process.env.NEXT_PUBLIC_SUPABASE_URL) return;
    setSaving(true);
    try {
      const supabase = createClient();
      await supabase.auth.updateUser({ data: { first_name: firstName, last_name: lastName } });
      onUserUpdate({ firstName, lastName });
      setSaved(true);
      setTimeout(() => setSaved(false), 2000);
    } finally { setSaving(false); }
  };

  const saveNotif = (key: string, val: boolean) => {
    const next = { ...notifPrefs, [key]: val };
    setNotifPrefs(next);
    try { localStorage.setItem("valori:notifs", JSON.stringify(next)); } catch {}
    if (key === "desktop" && val && "Notification" in window) {
      void Notification.requestPermission();
    }
  };

  const saveReranker = (update: Partial<{ provider: string; apiKey: string; model: string; endpoint: string }>) => {
    const next = { provider: update.provider ?? rerankerProvider, apiKey: update.apiKey ?? rerankerKey, model: update.model ?? rerankerModel, endpoint: update.endpoint ?? rerankerEndpoint };
    try { localStorage.setItem("valori:reranker_config", JSON.stringify(next)); } catch {}
    if (update.provider !== undefined) setRerankerProvider(update.provider as "none"|"cohere"|"custom");
    if (update.apiKey   !== undefined) setRerankerKey(update.apiKey);
    if (update.model    !== undefined) setRerankerModel(update.model);
    if (update.endpoint !== undefined) setRerankerEndpoint(update.endpoint);
  };

  const THEMES: { value: ThemePref; label: string; Icon: React.ElementType }[] = [
    { value: "system", label: "System", Icon: Monitor },
    { value: "light",  label: "Light",  Icon: Sun },
    { value: "dark",   label: "Dark",   Icon: Moon },
  ];

  const NOTIFS = [
    { key: "products",  label: "Product updates",     description: "New features and improvements" },
    { key: "projects",  label: "Project events",       description: "Status changes on your projects" },
    { key: "security",  label: "Security alerts",      description: "Sign-ins and account changes" },
    { key: "desktop",   label: "Desktop notifications", description: "Native OS notifications from the app" },
  ];

  return (
    <div className="flex flex-col gap-6">
      {/* Profile */}
      {process.env.NEXT_PUBLIC_SUPABASE_URL && (
        <SCard>
          <SCardHeader title="Profile" description="Your display name across the platform" />
          <div className="p-5 flex flex-col gap-4">
            <div className="grid grid-cols-2 gap-3">
              <div className="flex flex-col gap-1.5">
                <label className="text-xs font-medium text-muted-foreground">First name</label>
                <input value={firstName} onChange={e => setFirstName(e.target.value)}
                  className="rounded-lg border border-input bg-background px-3 py-2 text-sm text-foreground focus:outline-none focus:ring-1 focus:ring-[var(--v-accent-ring)]" />
              </div>
              <div className="flex flex-col gap-1.5">
                <label className="text-xs font-medium text-muted-foreground">Last name</label>
                <input value={lastName} onChange={e => setLastName(e.target.value)}
                  className="rounded-lg border border-input bg-background px-3 py-2 text-sm text-foreground focus:outline-none focus:ring-1 focus:ring-[var(--v-accent-ring)]" />
              </div>
            </div>
            <div className="flex justify-end">
              <SBtn variant="primary" onClick={saveProfile} disabled={saving}>
                {saved ? <span className="flex items-center gap-1.5"><Check size={11} /> Saved</span> : saving ? "Saving…" : "Save changes"}
              </SBtn>
            </div>
          </div>
        </SCard>
      )}

      {/* Appearance */}
      <SCard>
        <SCardHeader title="Appearance" description="Choose how Valori looks on your device" />
        <div className="p-5">
          <div className="grid grid-cols-3 gap-2">
            {THEMES.map(({ value, label, Icon }) => (
              <button
                key={value}
                onClick={() => setTheme(value)}
                className={cn(
                  "flex flex-col items-center gap-2 p-3 rounded-xl border transition-all text-xs font-medium",
                  pref === value
                    ? "border-[var(--v-accent)] bg-[var(--v-accent-muted)] text-foreground [box-shadow:0_0_0_1px_var(--v-accent)]"
                    : "border-border bg-background text-muted-foreground hover:border-border/80 hover:text-foreground",
                )}
              >
                <Icon size={18} className={pref === value ? "text-[var(--v-accent)]" : ""} />
                {label}
                {pref === value && <Check size={10} className="text-[var(--v-accent)]" />}
              </button>
            ))}
          </div>
        </div>
      </SCard>

      {/* Notifications */}
      <SCard>
        <SCardHeader title="Notifications" description="Control what you hear about" />
        {NOTIFS.map((n, i) => (
          <SRow key={n.key} label={n.label} description={n.description} last={i === NOTIFS.length - 1}>
            <Toggle checked={notifPrefs[n.key] !== false} onChange={v => saveNotif(n.key, v)} />
          </SRow>
        ))}
      </SCard>

      {/* Workspace */}
      <SCard>
        <SCardHeader title="Workspace" description="Local data folders managed by the desktop app" />
        <SRow label="Workspace folder" description={workspaceDir ?? "Not set"}>
          <div className="flex items-center gap-1.5">
            <SBtn onClick={() => workspaceDir && revealPath(workspaceDir)} disabled={!nativeAvailable() || !workspaceDir}>
              <FolderOpen size={11} className="inline mr-1" />Open
            </SBtn>
            <SBtn onClick={async () => {
              const dir = await pickFolder("Workspace");
              if (dir) { setWorkspaceDir(dir); await setPreference("workspaceDir", dir).catch(() => {}); }
            }} disabled={!nativeAvailable()}>
              Change
            </SBtn>
          </div>
        </SRow>
        {serverConfig && (
          <SRow label="Backend connection" description={serverConfig.api_url ?? "—"} last>
            {serverConfig.dim && (
              <span className="font-mono text-xs text-muted-foreground">dim {serverConfig.dim}</span>
            )}
          </SRow>
        )}
      </SCard>

      {/* Embedding Engine */}
      <SCard>
        <SCardHeader title="Embedding Engine" description="Provider used for automatic document ingestion" />
        <div className="p-5">
          <EmbeddingSelector />
        </div>
      </SCard>

      {/* Reasoning LLM */}
      <SCard>
        <SCardHeader title="Reasoning LLM" description="Model used for extraction and summarisation" />
        <div className="p-5">
          <LLMSelector />
        </div>
      </SCard>

      {/* Tier-2 Reranker */}
      <SCard>
        <SCardHeader title="Tier-2 Reranker" description="Cross-encoder re-rank after vector search" />
        <SRow label="Provider" last={rerankerProvider === "none"}>
          <div className="flex items-center gap-1.5">
            {(["none","cohere","custom"] as const).map(p => (
              <button key={p} onClick={() => saveReranker({ provider: p })}
                className={cn("px-2.5 py-1.5 text-xs rounded-lg border transition-colors",
                  rerankerProvider === p
                    ? "border-[var(--v-accent)] bg-[var(--v-accent-muted)] text-[var(--v-accent)]"
                    : "border-border bg-background text-muted-foreground hover:text-foreground hover:bg-accent")}>
                {p === "none" ? "Disabled" : p === "cohere" ? "Cohere" : "Custom"}
              </button>
            ))}
          </div>
        </SRow>
        {rerankerProvider === "cohere" && (
          <div className="px-5 pb-5 pt-4 border-t border-border/40 flex flex-col gap-3">
            <div className="flex flex-col gap-1.5">
              <label className="text-xs font-medium text-muted-foreground">API key</label>
              <input type="password" value={rerankerKey} onChange={e => saveReranker({ apiKey: e.target.value })}
                placeholder="co-…" className="rounded-lg border border-input bg-background px-3 py-2 text-sm focus:outline-none focus:ring-1 focus:ring-[var(--v-accent-ring)]" />
            </div>
            <div className="flex flex-col gap-1.5">
              <label className="text-xs font-medium text-muted-foreground">Model</label>
              <select value={rerankerModel} onChange={e => saveReranker({ model: e.target.value })}
                className="rounded-lg border border-input bg-background px-3 py-2 text-sm focus:outline-none focus:ring-1 focus:ring-[var(--v-accent-ring)]">
                <option>rerank-english-v3.0</option>
                <option>rerank-multilingual-v3.0</option>
                <option>rerank-english-v2.0</option>
              </select>
            </div>
          </div>
        )}
        {rerankerProvider === "custom" && (
          <div className="px-5 pb-5 pt-4 border-t border-border/40 flex flex-col gap-1.5">
            <label className="text-xs font-medium text-muted-foreground">Endpoint URL</label>
            <input value={rerankerEndpoint} onChange={e => saveReranker({ endpoint: e.target.value })}
              placeholder="http://localhost:8080/rerank"
              className="rounded-lg border border-input bg-background px-3 py-2 text-sm focus:outline-none focus:ring-1 focus:ring-[var(--v-accent-ring)]" />
          </div>
        )}
      </SCard>

      {/* Developer — desktop only */}
      {nativeAvailable() && (
        <SCard>
          <SCardHeader title="Developer" />
          <SRow label="Reset onboarding" description="Shows the welcome flow again on next launch" last>
            <SBtn variant="danger" onClick={async () => { await resetOnboarding(); window.location.reload(); }}>
              Reset
            </SBtn>
          </SRow>
        </SCard>
      )}
    </div>
  );
}

/* ─── Account section ───────────────────────────────────────────────────── */

function AccountSection({ user }: { user: UserData | null }) {
  const [sessions, setSessions]         = useState<SessionRow[]>([]);
  const [signingOutAll, setSigningOutAll] = useState(false);
  const [signingOutOthers, setSigningOutOthers] = useState(false);

  const loadSessions = useCallback(() => {
    if (!process.env.NEXT_PUBLIC_SUPABASE_URL) return;
    createClient().rpc("list_my_sessions").then(({ data }) => {
      if (data) setSessions(data as SessionRow[]);
    }, () => {});
  }, []);

  useEffect(() => { loadSessions(); }, [loadSessions]);

  // GoTrue's own "sign out other sessions" — scope: 'others' revokes every
  // session but this one using the caller's own access token. There is no
  // per-session terminate endpoint in this backend (see security/actions.ts),
  // so individual session rows are informational only.
  const signOutOthers = async () => {
    setSigningOutOthers(true);
    try {
      await createClient().auth.signOut({ scope: "others" });
      loadSessions();
    } finally { setSigningOutOthers(false); }
  };

  const signOutEverywhere = async () => {
    setSigningOutAll(true);
    try {
      await createClient().auth.signOut({ scope: "global" });
      window.location.href = "/";
    } finally { setSigningOutAll(false); }
  };

  const parseAgent = (ua: string | null) => {
    if (!ua) return { browser: "Unknown browser", os: "Unknown OS" };
    const browser =
      ua.includes("Valori")  ? "Valori Desktop" :
      ua.includes("Chrome")  ? "Chrome" :
      ua.includes("Safari")  ? "Safari" :
      ua.includes("Firefox") ? "Firefox" : "Browser";
    const os =
      ua.includes("Mac")     ? "macOS" :
      ua.includes("Windows") ? "Windows" :
      ua.includes("Linux")   ? "Linux" :
      ua.includes("iPhone") || ua.includes("iPad") ? "iOS" :
      ua.includes("Android") ? "Android" : "Unknown OS";
    return { browser, os };
  };

  const fmtDate = (iso: string) =>
    new Date(iso).toLocaleString(undefined, { month: "short", day: "numeric", hour: "2-digit", minute: "2-digit" });

  if (!process.env.NEXT_PUBLIC_SUPABASE_URL || !user) {
    return (
      <div className="text-sm text-muted-foreground">
        Sign in to view account details.
      </div>
    );
  }

  const joinedDate = user.createdAt
    ? new Date(user.createdAt).toLocaleDateString(undefined, { month: "long", year: "numeric" })
    : "—";

  return (
    <div className="flex flex-col gap-6">
      {/* Account info */}
      <SCard>
        <SCardHeader title="Account Information" />
        <SRow label="Email" last={!user.orgId}>
          <span className="text-xs text-muted-foreground font-mono">{user.email}</span>
        </SRow>
        {user.orgId && (
          <SRow label="Organization ID">
            <span className="text-xs text-muted-foreground font-mono truncate max-w-[180px]">{user.orgId}</span>
          </SRow>
        )}
        <SRow label="Joined" last>
          <span className="text-xs text-muted-foreground">{joinedDate}</span>
        </SRow>
      </SCard>

      {/* Active sessions */}
      <SCard>
        <div className="flex items-center justify-between gap-4 px-5 pt-5 pb-4 border-b border-border/60">
          <div>
            <h3 className="text-sm font-semibold text-foreground">Active Sessions</h3>
            <p className="text-xs text-muted-foreground mt-0.5">Devices currently signed in to your account</p>
          </div>
          {sessions.length > 1 && (
            <SBtn variant="danger" onClick={signOutOthers} disabled={signingOutOthers}>
              <LogOut size={11} className="inline mr-1" />
              {signingOutOthers ? "Signing out…" : "Sign out other sessions"}
            </SBtn>
          )}
        </div>
        <div className="divide-y divide-border/40">
          {sessions.length === 0 ? (
            <p className="px-5 py-4 text-sm text-muted-foreground">No session data available.</p>
          ) : sessions.map((s) => {
            const { browser, os } = parseAgent(s.user_agent);
            return (
              <div key={s.session_id}
                className={cn("flex items-start justify-between gap-4 px-5 py-4",
                  s.is_current && "border-l-2 border-l-emerald-500")}>
                <div className="min-w-0">
                  <div className="flex items-center gap-2 mb-0.5">
                    {s.is_current && <span className="h-2 w-2 rounded-full bg-emerald-500 shrink-0" />}
                    <span className="text-sm font-medium text-foreground">{browser}</span>
                    {s.is_current && <span className="text-[10px] text-emerald-600 dark:text-emerald-400 font-medium">Current</span>}
                  </div>
                  <p className="text-xs text-muted-foreground">{os}{s.ip ? ` · ${s.ip}` : ""}</p>
                  <p className="text-[11px] text-muted-foreground mt-0.5">Last active {fmtDate(s.updated_at)}</p>
                </div>
              </div>
            );
          })}
        </div>
      </SCard>

      {/* Security */}
      <SCard>
        <SCardHeader title="Security" />
        <SRow label="Sign out everywhere" description="Terminates all active sessions, including this one" last>
          <SBtn variant="danger" onClick={signOutEverywhere} disabled={signingOutAll}>
            <LogOut size={11} className="inline mr-1" />
            {signingOutAll ? "Signing out…" : "Sign out all"}
          </SBtn>
        </SRow>
      </SCard>

      {/* Danger zone */}
      <DangerCard>
        <div className="flex items-center justify-between gap-4 px-5 py-4">
          <div>
            <p className="text-sm font-medium text-foreground">Delete Account</p>
            <p className="text-[11px] text-muted-foreground mt-0.5">
              Account deletion isn&apos;t available yet — contact support to request it.
            </p>
          </div>
          <ComingSoon />
        </div>
      </DangerCard>
    </div>
  );
}

/* ─── Privacy section ───────────────────────────────────────────────────── */

function PrivacySection() {
  const [prefs, setPrefs] = useState(() => {
    try { return JSON.parse(localStorage.getItem("valori:privacy") ?? "{}"); } catch { return {}; }
  });
  const [clearing, setClearing] = useState(false);

  const save = (key: string, val: boolean) => {
    const next = { ...prefs, [key]: val };
    setPrefs(next);
    try { localStorage.setItem("valori:privacy", JSON.stringify(next)); } catch {}
  };

  const clearCache = async () => {
    setClearing(true);
    try {
      localStorage.removeItem("valori:reranker_config");
      localStorage.removeItem("valori:notifs");
      localStorage.removeItem("valori:privacy");
      window.location.reload();
    } finally { setClearing(false); }
  };

  const DATA_ITEMS = [
    { key: "diagnostics", label: "Anonymous diagnostics", description: "Helps us fix crashes and improve performance" },
    { key: "crash",       label: "Crash reports",         description: "Automatically send crash logs" },
    { key: "analytics",   label: "Product analytics",     description: "Usage patterns to improve the product" },
  ];

  return (
    <div className="flex flex-col gap-6">
      <SCard>
        <SCardHeader title="Data Collection" description="Control what Valori collects about your usage" />
        {DATA_ITEMS.map((item, i) => (
          <SRow key={item.key} label={item.label} description={item.description} last={i === DATA_ITEMS.length - 1}>
            <Toggle checked={prefs[item.key] !== false} onChange={v => save(item.key, v)} />
          </SRow>
        ))}
      </SCard>

      <SCard>
        <SCardHeader title="Your Data" description="Export or manage your stored data" />
        <SRow label="Export account data" description="Download everything Valori has stored for you" last>
          <ComingSoon />
        </SRow>
      </SCard>

      {nativeAvailable() && (
        <SCard>
          <SCardHeader title="Local Cache" description="Clear cached preferences and local indexes" />
          <SRow label="Clear preferences" description="Resets app settings to defaults" last>
            <SBtn variant="danger" onClick={clearCache} disabled={clearing}>
              <Trash2 size={11} className="inline mr-1" />
              {clearing ? "Clearing…" : "Clear"}
            </SBtn>
          </SRow>
        </SCard>
      )}
    </div>
  );
}

/* ─── Billing section ───────────────────────────────────────────────────── */

function BillingSection() {
  return (
    <div className="flex flex-col gap-6">
      <SCard>
        <SCardHeader title="Current Plan" />
        <div className="px-5 py-5 flex items-start justify-between gap-4">
          <div>
            <p className="text-lg font-bold text-foreground">Free</p>
            <p className="text-xs text-muted-foreground mt-0.5">Community tier — no credit card required</p>
          </div>
          <SBtn variant="primary" onClick={() => window.open("https://valori.systems/pricing", "_blank")}>
            <ArrowRight size={11} className="inline mr-1" />Upgrade
          </SBtn>
        </div>
      </SCard>

      <SCard>
        <SCardHeader title="Payment Method" description="No payment method on file" />
        <div className="px-5 py-4 text-sm text-muted-foreground">
          Upgrade to a paid plan to add a payment method.
        </div>
      </SCard>

      <SCard>
        <SCardHeader title="Invoices" description="Your billing history" />
        <div className="px-5 py-4 text-sm text-muted-foreground">
          No invoices yet.
        </div>
      </SCard>

    </div>
  );
}

/* ─── Usage section ─────────────────────────────────────────────────────── */

function UsageSection() {
  const { recordCount, fillPct, capacity, chainHeight } = useHealth();
  const [nodes, setNodes] = useState<number | null>(null);
  const [edges, setEdges] = useState<number | null>(null);

  useEffect(() => {
    fetch("/api/health").then(r => r.ok ? r.json() : null).then(d => {
      if (d?.nodes) setNodes(d.nodes.live ?? null);
      if (d?.edges) setEdges(d.edges.live ?? null);
    }).catch(() => {});
  }, []);

  const fmt = (n: number | null) =>
    n === null ? "—" : n >= 1_000_000 ? `${(n / 1_000_000).toFixed(1)}M` : n >= 1000 ? `${(n / 1000).toFixed(1)}K` : String(n);

  const pct = fillPct ?? 0;

  const METRICS = [
    { label: "Vectors",     value: fmt(recordCount),  Icon: Database },
    { label: "Nodes",       value: fmt(nodes),         Icon: Network },
    { label: "Edges",       value: fmt(edges),         Icon: Layers },
    { label: "Chain height",value: fmt(chainHeight),   Icon: ShieldCheck },
  ];

  return (
    <div className="flex flex-col gap-6">
      <SCard>
        <SCardHeader title="Vector Storage" description="Records in the current project" />
        <div className="px-5 pb-5 pt-4">
          <div className="flex items-center justify-between mb-2">
            <span className="text-xs text-muted-foreground">
              {fmt(recordCount)} / {fmt(capacity)} records
            </span>
            <span className="text-xs font-medium text-foreground">{pct.toFixed(1)}%</span>
          </div>
          <div className="h-2 rounded-full bg-border overflow-hidden">
            <div
              className={cn("h-full rounded-full transition-all duration-500",
                pct > 90 ? "bg-red-500" : pct > 70 ? "bg-amber-500" : "bg-[var(--v-accent)]")}
              style={{ width: `${Math.min(pct, 100)}%` }}
            />
          </div>
        </div>
      </SCard>

      <div className="grid grid-cols-2 gap-3">
        {METRICS.map(({ label, value, Icon }) => (
          <div key={label} className="rounded-xl border border-border bg-card p-4">
            <div className="flex items-center gap-2 mb-2">
              <Icon size={13} className="text-muted-foreground" />
              <span className="text-xs text-muted-foreground">{label}</span>
            </div>
            <p className="text-2xl font-bold text-foreground font-mono">{value}</p>
          </div>
        ))}
      </div>

      <SCard>
        <SCardHeader title="Object Store" description="Configure S3 / MinIO / R2 via environment variables" />
        <div className="px-5 py-4 border-t border-border/40">
          <div className="rounded-lg bg-accent/50 border border-border/60 px-4 py-3 font-mono text-xs text-muted-foreground space-y-1.5">
            <p><span className="text-[var(--v-accent)]">VALORI_OBJECT_STORE_URL</span>=s3://my-bucket/valori</p>
            <p><span className="text-[var(--v-accent)]">VALORI_OBJECT_STORE_REGION</span>=us-east-1</p>
            <p><span className="text-[var(--v-accent)]">VALORI_OBJECT_STORE_KEEP</span>=7</p>
          </div>
        </div>
        <SRow label="Browse snapshots" last>
          <a href="/snapshots" className="flex items-center gap-1 text-xs text-[var(--v-accent)] hover:underline">
            Open <ChevronRight size={11} />
          </a>
        </SRow>
      </SCard>
    </div>
  );
}

/* ─── About section ─────────────────────────────────────────────────────── */

function AboutSection() {
  const { version, status } = useHealth();

  const LINKS = [
    { label: "Website",       href: "https://valori.systems",                   Icon: ExternalLink },
    { label: "Documentation", href: "https://valori.systems/docs",              Icon: ExternalLink },
    { label: "GitHub",        href: "https://github.com/valori-ai",             Icon: ExternalLink },
    { label: "Changelog",     href: "https://valori.systems/changelog",         Icon: ExternalLink },
  ];

  return (
    <div className="flex flex-col gap-6">
      <SCard>
        <SCardHeader title="Version" />
        <SRow label="Kernel" last={!version}>
          <span className="font-mono text-xs text-muted-foreground">{version ?? "—"}</span>
        </SRow>
        {version && (
          <SRow label="Status" last>
            <span className={cn("text-xs font-medium",
              status === "ok" ? "text-emerald-600 dark:text-emerald-400" :
              status === "degraded" ? "text-amber-600 dark:text-amber-400" :
              "text-red-600 dark:text-red-400")}>
              {status ?? "—"}
            </span>
          </SRow>
        )}
      </SCard>

      <SCard>
        <SCardHeader title="Links" />
        {LINKS.map((link, i) => (
          <SRow key={link.label} label={link.label} last={i === LINKS.length - 1}>
            <a href={link.href} target="_blank" rel="noopener noreferrer"
              className="flex items-center gap-1 text-xs text-[var(--v-accent)] hover:underline">
              Open <link.Icon size={10} />
            </a>
          </SRow>
        ))}
      </SCard>

      <SCard>
        <SCardHeader title="Diagnostics" />
        <div className="px-5 py-4">
          <p className="text-xs text-muted-foreground mb-3">
            Copy diagnostic information to share with support.
          </p>
          <SBtn onClick={() => {
            const info = JSON.stringify({ version, status, ua: navigator.userAgent, ts: new Date().toISOString() }, null, 2);
            navigator.clipboard.writeText(info).catch(() => {});
          }}>
            Copy diagnostics
          </SBtn>
        </div>
      </SCard>
    </div>
  );
}

/* ─── Main modal ────────────────────────────────────────────────────────── */

export function SettingsModal({ open, onClose }: { open: boolean; onClose: () => void }) {
  const [activeSection, setActiveSection] = useState<SectionId>("general");
  const [search, setSearch] = useState("");
  const [user, setUser] = useState<UserData | null>(null);
  const searchRef = useRef<HTMLInputElement>(null);
  const contentRef = useRef<HTMLDivElement>(null);

  // Load user data once when modal opens
  useEffect(() => {
    if (!open || !process.env.NEXT_PUBLIC_SUPABASE_URL) return;
    (async () => {
      const supabase = createClient();
      const { data: { user: u } } = await supabase.auth.getUser();
      if (!u) return;
      const meta = u.user_metadata ?? {};
      const { data: memberships } = await supabase
        .from("org_members")
        .select("role, organizations(id, name)")
        .eq("user_id", u.id)
        .limit(1);
      const mem = memberships?.[0] as { role: string; organizations: { id: string; name: string } } | undefined;
      setUser({
        id: u.id,
        email: u.email ?? "",
        firstName: meta.first_name ?? meta.full_name?.split(" ")[0] ?? "",
        lastName:  meta.last_name  ?? meta.full_name?.split(" ").slice(1).join(" ") ?? "",
        avatarUrl: meta.avatar_url,
        provider:  u.app_metadata?.provider,
        createdAt: u.created_at,
        orgId:   mem?.organizations?.id,
        orgName: mem?.organizations?.name,
        role:    mem?.role,
      });
    })().catch(console.error);
  }, [open]);

  // Focus search on open, reset state on close
  useEffect(() => {
    if (open) {
      setTimeout(() => searchRef.current?.focus(), 50);
      contentRef.current?.scrollTo({ top: 0 });
    } else {
      setSearch("");
    }
  }, [open]);

  // ESC to close
  useEffect(() => {
    if (!open) return;
    const h = (e: KeyboardEvent) => { if (e.key === "Escape") onClose(); };
    window.addEventListener("keydown", h);
    return () => window.removeEventListener("keydown", h);
  }, [open, onClose]);

  // Scroll content to top on section change
  useEffect(() => { contentRef.current?.scrollTo({ top: 0, behavior: "smooth" }); }, [activeSection]);

  const filteredNav = NAV.filter(n => n.label.toLowerCase().includes(search.toLowerCase().trim()));

  const initials = user
    ? ((user.firstName[0] ?? "") + (user.lastName[0] ?? "")).toUpperCase() || user.email[0]?.toUpperCase() || "?"
    : "?";

  const displayName = user
    ? [user.firstName, user.lastName].filter(Boolean).join(" ") || user.email
    : "";

  const handleNavClick = useCallback((id: SectionId) => {
    setActiveSection(id);
    setSearch("");
  }, []);

  if (!open) return null;

  return (
    <div
      className="fixed inset-0 z-[200] flex items-center justify-center p-4"
      onClick={onClose}
    >
      {/* Backdrop — blurs and dims the page behind without hiding it */}
      <div className="absolute inset-0 bg-background/30 backdrop-blur-lg" />

      {/* Modal */}
      <div
        className="relative flex w-[min(92vw,1000px)] h-[min(85vh,720px)] rounded-2xl border border-border bg-card shadow-2xl overflow-hidden"
        onClick={e => e.stopPropagation()}
      >
        {/* ── Left panel ── */}
        <div className="w-[240px] shrink-0 flex flex-col border-r border-border bg-card/50">

          {/* Search */}
          <div className="p-3 border-b border-border/60">
            <div className="relative">
              <Search size={12} className="absolute left-2.5 top-1/2 -translate-y-1/2 text-muted-foreground pointer-events-none" />
              <input
                ref={searchRef}
                value={search}
                onChange={e => setSearch(e.target.value)}
                placeholder="Search settings…"
                className="w-full pl-7 pr-3 py-1.5 text-xs bg-background border border-border rounded-lg text-foreground placeholder:text-muted-foreground focus:outline-none focus:ring-1 focus:ring-[var(--v-accent-ring)] caret-[var(--v-accent)]"
              />
            </div>
          </div>

          {/* User profile */}
          {user && (
            <div className="px-3 py-3 border-b border-border/60">
              <div className="flex items-center gap-2.5">
                <div className="w-8 h-8 rounded-full bg-[var(--v-accent-muted)] flex items-center justify-center text-xs font-bold text-[var(--v-accent)] shrink-0">
                  {initials}
                </div>
                <div className="min-w-0">
                  <p className="text-xs font-semibold text-foreground truncate">{displayName}</p>
                  <div className="flex items-center gap-1.5 mt-0.5">
                    {user.role && (
                      <span className="text-[10px] text-muted-foreground capitalize">{user.role}</span>
                    )}
                    {user.role && <span className="text-[10px] text-border">·</span>}
                    <span className="text-[10px] text-muted-foreground">Free plan</span>
                  </div>
                </div>
              </div>
            </div>
          )}

          {/* Nav */}
          <nav className="flex-1 overflow-y-auto p-2 flex flex-col gap-0.5">
            {filteredNav.map(({ id, label, Icon }) => (
              <button
                key={id}
                onClick={() => handleNavClick(id)}
                className={cn(
                  "w-full flex items-center gap-2.5 px-2.5 py-2 rounded-lg text-sm font-medium transition-colors text-left",
                  activeSection === id && !search
                    ? "bg-[var(--v-accent-muted)] text-foreground [box-shadow:inset_2px_0_0_var(--v-accent)]"
                    : "text-muted-foreground hover:bg-accent/60 hover:text-foreground",
                )}
              >
                <Icon
                  size={14}
                  className={activeSection === id && !search ? "text-[var(--v-accent)]" : "text-muted-foreground"}
                  aria-hidden
                />
                {label}
              </button>
            ))}
            {filteredNav.length === 0 && (
              <p className="text-xs text-muted-foreground px-2.5 py-2">No results for &ldquo;{search}&rdquo;</p>
            )}
          </nav>
        </div>

        {/* ── Right panel ── */}
        <div className="flex-1 flex flex-col min-w-0">
          {/* Header */}
          <div className="flex items-center justify-between px-6 py-4 border-b border-border/60 bg-card/80 backdrop-blur-sm shrink-0">
            <div>
              <h1 className="text-sm font-semibold text-foreground">Settings</h1>
              <p className="text-[11px] text-muted-foreground">Manage your account, workspace and preferences</p>
            </div>
            <button
              onClick={onClose}
              aria-label="Close settings"
              className="flex h-7 w-7 items-center justify-center rounded-lg text-muted-foreground hover:bg-accent/70 hover:text-foreground transition-colors"
            >
              <X size={14} aria-hidden />
            </button>
          </div>

          {/* Scrollable content */}
          <div ref={contentRef} className="flex-1 overflow-y-auto px-6 py-6">
            {activeSection === "general" && (
              <GeneralSection user={user} onUserUpdate={u => setUser(prev => prev ? { ...prev, ...u } : prev)} />
            )}
            {activeSection === "account" && <AccountSection user={user} />}
            {activeSection === "privacy"  && <PrivacySection />}
            {activeSection === "billing"  && <BillingSection />}
            {activeSection === "usage"    && <UsageSection />}
            {activeSection === "about"    && <AboutSection />}
          </div>
        </div>
      </div>
    </div>
  );
}
