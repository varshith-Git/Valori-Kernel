"use client";

import { useState } from "react";
import Image from "next/image";
import { FolderOpen, ShieldCheck } from "lucide-react";
import { Button } from "@/components/ui/button";
import { markOnboardingComplete, pickFolder, setPreference, startDaemon } from "@/lib/native";

interface FolderFieldProps {
  label: string;
  help: string;
  value: string | null;
  onPick: (path: string) => void;
  optional?: boolean;
}

function FolderField({ label, help, value, onPick, optional }: FolderFieldProps) {
  const pick = async () => {
    const dir = await pickFolder(label);
    if (dir) onPick(dir);
  };
  return (
    <div className="flex flex-col gap-1.5">
      <div className="flex items-baseline gap-2">
        <span className="text-sm font-medium text-foreground">{label}</span>
        {optional && <span className="text-xs text-muted-foreground">(optional)</span>}
      </div>
      <p className="text-xs text-muted-foreground leading-relaxed">{help}</p>
      <div className="flex gap-2">
        <input
          readOnly
          value={value ?? ""}
          placeholder="Not selected"
          className="flex-1 rounded-lg border border-input bg-background px-3 py-2 text-xs font-mono text-foreground placeholder:text-muted-foreground"
        />
        <Button type="button" variant="outline" size="sm" onClick={pick} className="gap-1.5 shrink-0">
          <FolderOpen className="h-3.5 w-3.5" />
          Browse…
        </Button>
      </div>
    </div>
  );
}

const EXPLAINERS: { title: string; body: string }[] = [
  { title: "Projects", body: "Each project is an isolated Valori node — its own data, its own lifecycle." },
  { title: "Collections", body: "Namespaces inside a project that group related vectors and graph nodes." },
  { title: "Models", body: "Embedding models used to turn your data into vectors. Managed centrally, once installed." },
  { title: "Privacy", body: "All data stays on this machine unless you configure a remote provider yourself." },
];

export default function Welcome({ onFinish }: { onFinish: () => void }) {
  const [workspaceDir, setWorkspaceDir] = useState<string | null>(null);
  const [modelDir, setModelDir] = useState<string | null>(null);
  const [telemetry, setTelemetry] = useState(false);
  const [termsAccepted, setTermsAccepted] = useState(false);
  const [finishing, setFinishing] = useState(false);
  const [daemonError, setDaemonError] = useState<string | null>(null);

  const canFinish = !!workspaceDir && termsAccepted && !finishing;

  const finish = async () => {
    setFinishing(true);
    setDaemonError(null);
    try {
      await setPreference("workspaceDir", workspaceDir);
      await setPreference("modelDir", modelDir);
      await setPreference("telemetryEnabled", telemetry);
      await setPreference("termsAccepted", true);
      await markOnboardingComplete();
      // The workspace folder the user just picked becomes VALORI_HOME — this is
      // what makes the folder picker do something real instead of being cosmetic.
      await startDaemon(workspaceDir);
      onFinish();
    } catch (e) {
      setDaemonError(e instanceof Error ? e.message : "Failed to start daemon. Try again.");
      setFinishing(false);
    }
  };

  return (
    <div className="flex h-full w-full items-center justify-center bg-background p-6">
      <div className="w-full max-w-lg rounded-2xl border border-border/80 bg-card shadow-xl">
        <div className="flex items-center gap-3 px-8 pt-8">
          <div className="flex h-9 w-9 items-center justify-center rounded-xl bg-[var(--v-accent-muted)] border border-[var(--v-accent)]/30">
            <Image
              src="/logo.png"
              alt="Valori"
              width={28}
              height={28}
              className="dark:invert"
            />
          </div>
          <div>
            <h1 className="text-lg font-semibold text-foreground tracking-tight">Welcome to Valori</h1>
            <p className="text-xs text-muted-foreground">A verifiable memory system for AI agents.</p>
          </div>
        </div>

        <div className="flex flex-col gap-5 px-8 py-6">
          <FolderField
            label="Workspace folder"
            help="Where your project data lives on disk."
            value={workspaceDir}
            onPick={setWorkspaceDir}
          />
          <FolderField
            label="Model folder"
            help="Where downloaded embedding models are cached."
            value={modelDir}
            onPick={setModelDir}
            optional
          />

          <div className="rounded-xl border border-border/60 bg-background/60 p-4 flex flex-col gap-2.5">
            {EXPLAINERS.map((e) => (
              <p key={e.title} className="text-xs leading-relaxed">
                <span className="font-semibold text-foreground">{e.title}: </span>
                <span className="text-muted-foreground">{e.body}</span>
              </p>
            ))}
          </div>

          <label className="flex items-start gap-2.5 text-xs text-muted-foreground">
            <input
              type="checkbox"
              checked={telemetry}
              onChange={(e) => setTelemetry(e.target.checked)}
              className="mt-0.5"
            />
            <span>Share anonymous usage telemetry to help improve Valori (off by default)</span>
          </label>

          <label className="flex items-start gap-2.5 text-xs text-muted-foreground">
            <input
              type="checkbox"
              checked={termsAccepted}
              onChange={(e) => setTermsAccepted(e.target.checked)}
              className="mt-0.5"
            />
            <span>
              I agree to the{" "}
              <a
                href="https://github.com/valori-db/valori-kernel/blob/main/LICENSE-APACHE"
                target="_blank"
                rel="noreferrer"
                className="text-foreground underline underline-offset-2 hover:text-[var(--v-accent)]"
              >
                Apache 2.0
              </a>{" "}
              /{" "}
              <a
                href="https://github.com/valori-db/valori-kernel/blob/main/LICENSE-MIT"
                target="_blank"
                rel="noreferrer"
                className="text-foreground underline underline-offset-2 hover:text-[var(--v-accent)]"
              >
                MIT
              </a>{" "}
              license terms
            </span>
          </label>
        </div>

        {daemonError && (
          <div className="mx-8 mb-1 rounded-lg border border-destructive/40 bg-destructive/10 px-3 py-2 text-xs text-destructive">
            {daemonError}
          </div>
        )}
        <div className="flex items-center justify-between gap-3 border-t border-border/60 px-8 py-5">
          <span className="flex items-center gap-1.5 text-[11px] text-muted-foreground">
            <ShieldCheck className="h-3.5 w-3.5" />
            Nothing leaves this machine until you configure it to.
          </span>
          <Button type="button" disabled={!canFinish} onClick={finish} className="shrink-0">
            {finishing ? "Setting up…" : "Get started"}
          </Button>
        </div>
      </div>
    </div>
  );
}
