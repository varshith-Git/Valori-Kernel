"use client";

import { useState } from "react";
import Image from "next/image";
import { FolderOpen, Monitor, Package, ShieldCheck, Check } from "lucide-react";
import { Button } from "@/components/ui/button";
import {
  markOnboardingComplete,
  pickFolder,
  setPreference,
  startDaemon,
} from "@/lib/native";

// ── Types ─────────────────────────────────────────────────────────────────────

type StepId = "terms" | "folder" | "preferences" | "install";

interface Step {
  id: StepId;
  label: string;
  icon: React.ReactNode;
}

// ── Steps definition ──────────────────────────────────────────────────────────

const STEPS: Step[] = [
  { id: "terms",       label: "Terms & conditions", icon: <ShieldCheck size={13} /> },
  { id: "folder",      label: "Installation folder", icon: <FolderOpen size={13} /> },
  { id: "preferences", label: "Desktop icon",        icon: <Monitor size={13} /> },
  { id: "install",     label: "Install",             icon: <Package size={13} /> },
];

// ── Sidebar ───────────────────────────────────────────────────────────────────

function Sidebar({
  current,
  completed,
}: {
  current: StepId;
  completed: Set<StepId>;
}) {
  const currentIdx = STEPS.findIndex((s) => s.id === current);

  return (
    <aside
      style={{
        width: 240,
        minWidth: 240,
        display: "flex",
        flexDirection: "column",
        borderRight: "1px solid var(--border)",
        background: "var(--card)",
      }}
    >
      {/* Header */}
      <div
        style={{
          display: "flex",
          alignItems: "center",
          gap: 10,
          padding: "20px 18px 18px",
          borderBottom: "1px solid var(--border)",
        }}
      >
        <div
          style={{
            width: 32,
            height: 32,
            borderRadius: 8,
            background: "var(--v-accent-muted)",
            border: "1px solid color-mix(in oklch, var(--v-accent) 30%, transparent)",
            display: "flex",
            alignItems: "center",
            justifyContent: "center",
            flexShrink: 0,
          }}
        >
          <Image src="/logo.png" alt="Valori" width={20} height={20} className="dark:invert" />
        </div>
        <div style={{ display: "flex", alignItems: "baseline", gap: 6, minWidth: 0 }}>
          <span style={{ fontSize: 14, fontWeight: 600, color: "var(--foreground)" }}>
            valori studio
          </span>
          <span style={{ fontSize: 11, color: "var(--muted-foreground)", borderLeft: "1px solid var(--border)", paddingLeft: 6 }}>
            Setup
          </span>
        </div>
      </div>

      {/* Steps */}
      <nav style={{ flex: 1, padding: "12px 10px" }}>
        {STEPS.map((step, idx) => {
          const isActive    = step.id === current;
          const isDone      = completed.has(step.id);
          const isReachable = idx <= currentIdx;

          return (
            <div
              key={step.id}
              style={{
                display: "flex",
                alignItems: "center",
                gap: 10,
                padding: "8px 10px",
                borderRadius: 8,
                marginBottom: 2,
                background: isActive ? "var(--v-accent-muted)" : "transparent",
                cursor: isReachable ? "default" : "default",
              }}
            >
              {/* Checkbox square */}
              <div
                style={{
                  width: 18,
                  height: 18,
                  borderRadius: 4,
                  border: `1.5px solid ${
                    isDone
                      ? "var(--v-accent)"
                      : isActive
                      ? "var(--v-accent)"
                      : "var(--border)"
                  }`,
                  background: isDone ? "var(--v-accent)" : "transparent",
                  display: "flex",
                  alignItems: "center",
                  justifyContent: "center",
                  flexShrink: 0,
                  transition: "all 0.15s",
                }}
              >
                {isDone && <Check size={11} color="white" strokeWidth={3} />}
              </div>

              <span
                style={{
                  fontSize: 13,
                  fontWeight: isActive ? 500 : 400,
                  color: isActive
                    ? "var(--v-accent)"
                    : isDone
                    ? "var(--muted-foreground)"
                    : idx < currentIdx
                    ? "var(--muted-foreground)"
                    : "var(--muted-foreground)",
                }}
              >
                {step.label}
              </span>
            </div>
          );
        })}
      </nav>

      {/* Footer card */}
      <div
        style={{
          margin: "0 10px 14px",
          borderRadius: 10,
          background: "var(--v-accent-muted)",
          border: "1px solid color-mix(in oklch, var(--v-accent) 20%, transparent)",
          padding: "10px 12px",
          display: "flex",
          alignItems: "flex-start",
          gap: 8,
        }}
      >
        <div
          style={{
            width: 22,
            height: 22,
            borderRadius: 5,
            background: "var(--v-accent-muted)",
            border: "1px solid color-mix(in oklch, var(--v-accent) 30%, transparent)",
            display: "flex",
            alignItems: "center",
            justifyContent: "center",
            flexShrink: 0,
            marginTop: 1,
          }}
        >
          <ShieldCheck size={12} color="var(--v-accent)" />
        </div>
        <p style={{ fontSize: 11, color: "var(--muted-foreground)", lineHeight: 1.5, margin: 0 }}>
          Your privacy and trust are our priority.
        </p>
      </div>
    </aside>
  );
}

// ── Step panels ───────────────────────────────────────────────────────────────

const TERMS_SECTIONS = [
  {
    title: "Acceptance of terms",
    body: "By installing or using this software, you agree to comply with these Terms and all applicable laws. If installing on behalf of an organization, you represent that you have authority to bind that organization.",
  },
  {
    title: "Eligibility",
    body: "You must be at least 18 years old or have the consent of a parent or guardian. By continuing, you affirm that you meet these requirements.",
  },
  {
    title: "License",
    body: "Valori is dual-licensed under Apache 2.0 and MIT. You may use, copy, modify, and distribute the software under the terms of either license. Attribution is required when redistributing.",
  },
  {
    title: "Data & privacy",
    body: "All your data stays on this machine by default. Nothing is transmitted to external servers unless you explicitly configure a remote provider. Anonymous usage telemetry is opt-in only.",
  },
  {
    title: "No warranty",
    body: "This software is provided \"as is\", without warranty of any kind. The authors are not liable for any damages arising from use of this software.",
  },
];

function TermsPanel({
  agreed,
  onAgree,
  onDecline,
  onAccept,
}: {
  agreed: boolean;
  onAgree: (v: boolean) => void;
  onDecline: () => void;
  onAccept: () => void;
}) {
  return (
    <>
      <div style={{ flex: 1, overflowY: "auto", padding: "28px 32px" }}>
        {/* Welcome card */}
        <div
          style={{
            borderRadius: 12,
            background: "var(--v-accent-muted)",
            border: "1px solid color-mix(in oklch, var(--v-accent) 20%, transparent)",
            padding: "18px 20px",
            display: "flex",
            alignItems: "flex-start",
            gap: 14,
            marginBottom: 28,
          }}
        >
          <div
            style={{
              width: 44,
              height: 44,
              borderRadius: 10,
              background: "var(--v-accent-muted)",
              border: "1px solid color-mix(in oklch, var(--v-accent) 30%, transparent)",
              display: "flex",
              alignItems: "center",
              justifyContent: "center",
              flexShrink: 0,
            }}
          >
            <Image src="/logo.png" alt="Valori" width={28} height={28} className="dark:invert" />
          </div>
          <div>
            <p style={{ fontSize: 15, fontWeight: 600, color: "var(--v-accent)", margin: "0 0 4px" }}>
              Welcome!
            </p>
            <p style={{ fontSize: 13, color: "var(--muted-foreground)", lineHeight: 1.55, margin: 0 }}>
              These Terms and Conditions govern your use of Valori Studio and related services.
              Please read them carefully. By installing, you agree to be bound by these Terms.
            </p>
          </div>
        </div>

        {/* T&C sections */}
        <div style={{ display: "flex", flexDirection: "column", gap: 0 }}>
          {TERMS_SECTIONS.map((s, i) => (
            <div
              key={s.title}
              style={{
                display: "flex",
                gap: 16,
                paddingBottom: 24,
                borderBottom: i < TERMS_SECTIONS.length - 1
                  ? "1px solid var(--border)"
                  : "none",
                marginBottom: i < TERMS_SECTIONS.length - 1 ? 24 : 0,
              }}
            >
              <div
                style={{
                  width: 28,
                  height: 28,
                  borderRadius: 8,
                  background: "var(--v-accent-muted)",
                  border: "1px solid color-mix(in oklch, var(--v-accent) 20%, transparent)",
                  display: "flex",
                  alignItems: "center",
                  justifyContent: "center",
                  flexShrink: 0,
                  fontSize: 12,
                  fontWeight: 600,
                  color: "var(--v-accent)",
                }}
              >
                {i + 1}
              </div>
              <div>
                <p style={{ fontSize: 15, fontWeight: 600, color: "var(--foreground)", margin: "0 0 8px" }}>
                  {s.title}
                </p>
                <p style={{ fontSize: 13, color: "var(--muted-foreground)", lineHeight: 1.6, margin: 0 }}>
                  {s.body}
                </p>
              </div>
            </div>
          ))}
        </div>
      </div>

      {/* Bottom action bar */}
      <div
        style={{
          borderTop: "1px solid var(--border)",
          padding: "14px 32px",
          display: "flex",
          alignItems: "center",
          gap: 16,
          background: "var(--card)",
          flexShrink: 0,
        }}
      >
        <label
          style={{
            display: "flex",
            alignItems: "center",
            gap: 8,
            flex: 1,
            cursor: "pointer",
          }}
        >
          <input
            type="checkbox"
            checked={agreed}
            onChange={(e) => onAgree(e.target.checked)}
            style={{ width: 15, height: 15, cursor: "pointer", accentColor: "var(--v-accent)" }}
          />
          <span style={{ fontSize: 13, color: "var(--foreground)" }}>
            I have read and agree to the Terms &amp; Conditions
          </span>
        </label>
        <Button variant="outline" size="sm" onClick={onDecline}>
          Decline
        </Button>
        <Button size="sm" disabled={!agreed} onClick={onAccept}>
          Accept
        </Button>
      </div>
    </>
  );
}

function FolderPanel({
  workspaceDir,
  modelDir,
  onPickWorkspace,
  onPickModel,
  onBack,
  onNext,
}: {
  workspaceDir: string | null;
  modelDir: string | null;
  onPickWorkspace: () => void;
  onPickModel: () => void;
  onBack: () => void;
  onNext: () => void;
}) {
  return (
    <>
      <div style={{ flex: 1, overflowY: "auto", padding: "28px 32px" }}>
        <p style={{ fontSize: 13, color: "var(--muted-foreground)", margin: "0 0 28px", lineHeight: 1.6 }}>
          Choose where Valori stores your project data. The workspace folder is required;
          the model folder (for embedding caches) is optional.
        </p>

        {/* Workspace */}
        <div style={{ marginBottom: 24 }}>
          <p style={{ fontSize: 13, fontWeight: 600, color: "var(--foreground)", margin: "0 0 4px" }}>
            Workspace folder <span style={{ color: "var(--v-accent)" }}>*</span>
          </p>
          <p style={{ fontSize: 12, color: "var(--muted-foreground)", margin: "0 0 10px", lineHeight: 1.5 }}>
            Where your project data and audit logs live on disk.
          </p>
          <div style={{ display: "flex", gap: 8 }}>
            <input
              readOnly
              value={workspaceDir ?? ""}
              placeholder="Not selected"
              style={{
                flex: 1,
                borderRadius: 8,
                border: "1px solid var(--border)",
                background: "var(--background)",
                padding: "8px 12px",
                fontSize: 12,
                fontFamily: "ui-monospace, monospace",
                color: workspaceDir ? "var(--foreground)" : "var(--muted-foreground)",
                outline: "none",
              }}
            />
            <Button type="button" variant="outline" size="sm" onClick={onPickWorkspace}
              style={{ gap: 6, flexShrink: 0 }}>
              <FolderOpen size={13} />
              Browse…
            </Button>
          </div>
        </div>

        {/* Model dir */}
        <div>
          <p style={{ fontSize: 13, fontWeight: 600, color: "var(--foreground)", margin: "0 0 4px" }}>
            Model folder{" "}
            <span style={{ fontSize: 11, fontWeight: 400, color: "var(--muted-foreground)" }}>(optional)</span>
          </p>
          <p style={{ fontSize: 12, color: "var(--muted-foreground)", margin: "0 0 10px", lineHeight: 1.5 }}>
            Where downloaded embedding models are cached. Defaults to the workspace folder if left blank.
          </p>
          <div style={{ display: "flex", gap: 8 }}>
            <input
              readOnly
              value={modelDir ?? ""}
              placeholder="Same as workspace"
              style={{
                flex: 1,
                borderRadius: 8,
                border: "1px solid var(--border)",
                background: "var(--background)",
                padding: "8px 12px",
                fontSize: 12,
                fontFamily: "ui-monospace, monospace",
                color: modelDir ? "var(--foreground)" : "var(--muted-foreground)",
                outline: "none",
              }}
            />
            <Button type="button" variant="outline" size="sm" onClick={onPickModel}
              style={{ gap: 6, flexShrink: 0 }}>
              <FolderOpen size={13} />
              Browse…
            </Button>
          </div>
        </div>
      </div>

      <div
        style={{
          borderTop: "1px solid var(--border)",
          padding: "14px 32px",
          display: "flex",
          justifyContent: "space-between",
          background: "var(--card)",
          flexShrink: 0,
        }}
      >
        <Button variant="outline" size="sm" onClick={onBack}>Back</Button>
        <Button size="sm" disabled={!workspaceDir} onClick={onNext}>Continue</Button>
      </div>
    </>
  );
}

function PreferencesPanel({
  telemetry,
  onTelemetry,
  dockIcon,
  onDockIcon,
  onBack,
  onNext,
}: {
  telemetry: boolean;
  onTelemetry: (v: boolean) => void;
  dockIcon: boolean;
  onDockIcon: (v: boolean) => void;
  onBack: () => void;
  onNext: () => void;
}) {
  const ToggleRow = ({
    label,
    desc,
    checked,
    onChange,
  }: {
    label: string;
    desc: string;
    checked: boolean;
    onChange: (v: boolean) => void;
  }) => (
    <label
      style={{
        display: "flex",
        alignItems: "flex-start",
        gap: 12,
        cursor: "pointer",
        padding: "14px 0",
        borderBottom: "1px solid var(--border)",
      }}
    >
      <div style={{ paddingTop: 2 }}>
        <input
          type="checkbox"
          checked={checked}
          onChange={(e) => onChange(e.target.checked)}
          style={{ width: 15, height: 15, cursor: "pointer", accentColor: "var(--v-accent)" }}
        />
      </div>
      <div>
        <p style={{ fontSize: 13, fontWeight: 500, color: "var(--foreground)", margin: "0 0 3px" }}>{label}</p>
        <p style={{ fontSize: 12, color: "var(--muted-foreground)", margin: 0, lineHeight: 1.5 }}>{desc}</p>
      </div>
    </label>
  );

  return (
    <>
      <div style={{ flex: 1, overflowY: "auto", padding: "28px 32px" }}>
        <p style={{ fontSize: 13, color: "var(--muted-foreground)", margin: "0 0 20px", lineHeight: 1.6 }}>
          Customise how Valori behaves on this machine.
        </p>

        <ToggleRow
          label="Keep Valori in the Dock"
          desc="Pin the app to your Dock so it's always one click away. You can change this later in System Settings."
          checked={dockIcon}
          onChange={onDockIcon}
        />
        <ToggleRow
          label="Share anonymous usage telemetry"
          desc="Sends crash reports and feature usage counts — no personal data, no vector content. Helps us prioritise fixes."
          checked={telemetry}
          onChange={onTelemetry}
        />
      </div>

      <div
        style={{
          borderTop: "1px solid var(--border)",
          padding: "14px 32px",
          display: "flex",
          justifyContent: "space-between",
          background: "var(--card)",
          flexShrink: 0,
        }}
      >
        <Button variant="outline" size="sm" onClick={onBack}>Back</Button>
        <Button size="sm" onClick={onNext}>Continue</Button>
      </div>
    </>
  );
}

function InstallPanel({
  workspaceDir,
  installing,
  error,
  onBack,
  onInstall,
}: {
  workspaceDir: string | null;
  installing: boolean;
  error: string | null;
  onBack: () => void;
  onInstall: () => void;
}) {
  return (
    <>
      <div style={{ flex: 1, overflowY: "auto", padding: "28px 32px" }}>
        <p style={{ fontSize: 13, color: "var(--muted-foreground)", margin: "0 0 24px", lineHeight: 1.6 }}>
          Everything is ready. Click <strong>Install</strong> to set up Valori and start the first session.
        </p>

        {/* Summary */}
        <div
          style={{
            borderRadius: 10,
            border: "1px solid var(--border)",
            background: "var(--background)",
            overflow: "hidden",
            marginBottom: 20,
          }}
        >
          {[
            { label: "Workspace", value: workspaceDir ?? "—" },
            { label: "Telemetry", value: "Off" },
          ].map((row, i, arr) => (
            <div
              key={row.label}
              style={{
                display: "flex",
                alignItems: "flex-start",
                gap: 12,
                padding: "10px 16px",
                borderBottom: i < arr.length - 1 ? "1px solid var(--border)" : "none",
              }}
            >
              <span style={{ fontSize: 12, color: "var(--muted-foreground)", width: 90, flexShrink: 0 }}>
                {row.label}
              </span>
              <span
                style={{
                  fontSize: 12,
                  fontFamily: "ui-monospace, monospace",
                  color: "var(--foreground)",
                  wordBreak: "break-all",
                }}
              >
                {row.value}
              </span>
            </div>
          ))}
        </div>

        {error && (
          <div
            style={{
              borderRadius: 8,
              border: "1px solid color-mix(in oklch, var(--destructive) 40%, transparent)",
              background: "color-mix(in oklch, var(--destructive) 10%, transparent)",
              padding: "10px 14px",
              fontSize: 12,
              color: "var(--destructive)",
              lineHeight: 1.5,
            }}
          >
            {error}
          </div>
        )}
      </div>

      <div
        style={{
          borderTop: "1px solid var(--border)",
          padding: "14px 32px",
          display: "flex",
          justifyContent: "space-between",
          background: "var(--card)",
          flexShrink: 0,
        }}
      >
        <Button variant="outline" size="sm" onClick={onBack} disabled={installing}>Back</Button>
        <Button size="sm" disabled={installing} onClick={onInstall}>
          {installing ? "Setting up…" : "Install"}
        </Button>
      </div>
    </>
  );
}

// ── Main Welcome component ────────────────────────────────────────────────────

export default function Welcome({ onFinish }: { onFinish: () => void }) {
  const [step,         setStep]         = useState<StepId>("terms");
  const [completed,    setCompleted]     = useState<Set<StepId>>(new Set());
  const [agreed,       setAgreed]        = useState(false);
  const [workspaceDir, setWorkspaceDir]  = useState<string | null>(null);
  const [modelDir,     setModelDir]      = useState<string | null>(null);
  const [telemetry,    setTelemetry]     = useState(false);
  const [dockIcon,     setDockIcon]      = useState(true);
  const [installing,   setInstalling]    = useState(false);
  const [installError, setInstallError]  = useState<string | null>(null);

  const markDone = (id: StepId) =>
    setCompleted((prev) => new Set([...prev, id]));

  const goTo = (id: StepId) => setStep(id);

  const handleAccept = () => { markDone("terms"); goTo("folder"); };
  const handleDecline = () => {
    // Nothing to persist — just re-show the terms.
    setAgreed(false);
  };

  const handlePickWorkspace = async () => {
    const dir = await pickFolder("Choose workspace folder");
    if (dir) setWorkspaceDir(dir);
  };

  const handlePickModel = async () => {
    const dir = await pickFolder("Choose model folder");
    if (dir) setModelDir(dir);
  };

  const handleFolderNext = () => { markDone("folder"); goTo("preferences"); };
  const handlePrefsNext  = () => { markDone("preferences"); goTo("install"); };

  const handleInstall = async () => {
    setInstalling(true);
    setInstallError(null);
    try {
      await setPreference("workspaceDir",    workspaceDir);
      await setPreference("modelDir",        modelDir);
      await setPreference("telemetryEnabled", telemetry);
      await setPreference("dockIcon",        dockIcon);
      await setPreference("termsAccepted",   true);
      await markOnboardingComplete();
      await startDaemon(workspaceDir);
      markDone("install");
      onFinish();
    } catch (e) {
      setInstallError(
        e instanceof Error ? e.message : "Setup failed. Please quit and try again.",
      );
      setInstalling(false);
    }
  };

  const currentStepLabel = STEPS.find((s) => s.id === step)?.label ?? "";

  return (
    <div
      style={{
        display: "flex",
        height: "100%",
        width: "100%",
        background: "var(--background)",
        fontFamily: "-apple-system, BlinkMacSystemFont, 'Segoe UI', sans-serif",
      }}
    >
      <Sidebar current={step} completed={completed} />

      {/* Main panel */}
      <div style={{ flex: 1, display: "flex", flexDirection: "column", minWidth: 0 }}>
        {/* Panel header */}
        <div
          style={{
            padding: "20px 32px 18px",
            borderBottom: "1px solid var(--border)",
            flexShrink: 0,
          }}
        >
          <h1 style={{ fontSize: 18, fontWeight: 600, color: "var(--foreground)", margin: 0 }}>
            {currentStepLabel}
          </h1>
        </div>

        {/* Step content */}
        {step === "terms" && (
          <TermsPanel
            agreed={agreed}
            onAgree={setAgreed}
            onDecline={handleDecline}
            onAccept={handleAccept}
          />
        )}
        {step === "folder" && (
          <FolderPanel
            workspaceDir={workspaceDir}
            modelDir={modelDir}
            onPickWorkspace={handlePickWorkspace}
            onPickModel={handlePickModel}
            onBack={() => goTo("terms")}
            onNext={handleFolderNext}
          />
        )}
        {step === "preferences" && (
          <PreferencesPanel
            telemetry={telemetry}
            onTelemetry={setTelemetry}
            dockIcon={dockIcon}
            onDockIcon={setDockIcon}
            onBack={() => goTo("folder")}
            onNext={handlePrefsNext}
          />
        )}
        {step === "install" && (
          <InstallPanel
            workspaceDir={workspaceDir}
            installing={installing}
            error={installError}
            onBack={() => goTo("preferences")}
            onInstall={handleInstall}
          />
        )}
      </div>
    </div>
  );
}
