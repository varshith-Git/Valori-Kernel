"use client";

import { useState, useEffect } from "react";
import Link from "next/link";
import { Check, X } from "lucide-react";
import type { ManifestProject } from "@/lib/hooks/useProjectManifest";
import { getOnboardingFlags, dismissOnboarding } from "@/lib/onboarding";

interface Step {
  title: string;
  description: string;
  done: boolean;
  href?: string;
  onClick?: () => void;
}

interface Props {
  projects: ManifestProject[];
  recordCount: number | null;
  onCreateProject: () => void;
}

export function GettingStarted({ projects, recordCount, onCreateProject }: Props) {
  // localStorage is read after mount so SSR and client render match.
  const [flags, setFlags] = useState({ searched: false, proof: false, dismissed: true });
  useEffect(() => { setFlags({ ...getOnboardingFlags() }); }, []);

  const running = projects.find((p) => p.status === "running");
  const runningHref = running
    ? `/projects/${encodeURIComponent(running.name)}/${encodeURIComponent(running.collections?.[0] ?? "default")}`
    : undefined;

  const steps: Step[] = [
    {
      title: "Create a project",
      description: "A project is an isolated store with its own dimension, index, and audit chain.",
      done: projects.length > 0,
      onClick: onCreateProject,
    },
    {
      title: "Open a project",
      description: "Opening starts the node session — your data persists between restarts.",
      done: !!running,
    },
    {
      title: "Insert your first vector",
      description: "Paste vectors in Bulk Insert, or upload a document to chunk + embed it.",
      done: (recordCount ?? 0) > 0,
      href: runningHref,
    },
    {
      title: "Run a search",
      description: "Semantic, hybrid, by-ID, or find-similar — every result is reproducible.",
      done: flags.searched,
      href: runningHref,
    },
    {
      title: "Verify your first proof",
      description: "The BLAKE3 state hash proves exactly what your data is — no other vector DB can.",
      done: flags.proof,
      href: "/proof",
    },
  ];

  const doneCount = steps.filter((s) => s.done).length;
  const allDone = doneCount === steps.length;

  if (flags.dismissed || allDone) return null;

  return (
    <div className="animate-fade-up rounded-xl border border-border bg-card p-5" style={{ animationDelay: "40ms" }}>
      <div className="flex items-start justify-between mb-4">
        <div>
          <h2 className="text-base font-semibold text-foreground">Let&apos;s get started</h2>
          <p className="mt-0.5 text-xs text-muted-foreground">
            {doneCount} of {steps.length} complete
          </p>
        </div>
        <button
          onClick={() => { dismissOnboarding(); setFlags((f) => ({ ...f, dismissed: true })); }}
          className="rounded-md p-1 text-muted-foreground hover:text-foreground hover:bg-accent transition-colors"
          title="Dismiss"
        >
          <X size={14} />
        </button>
      </div>

      {/* Progress bar */}
      <div className="h-1 rounded-full bg-accent mb-5 overflow-hidden">
        <div
          className="h-full rounded-full bg-[var(--v-accent)] transition-all duration-500"
          style={{ width: `${(doneCount / steps.length) * 100}%` }}
        />
      </div>

      <div className="flex flex-col gap-1">
        {steps.map((step, i) => {
          const inner = (
            <div
              className={`flex items-start gap-3 rounded-lg px-3 py-2.5 transition-colors ${
                step.done ? "opacity-60" : (step.href || step.onClick) ? "hover:bg-accent cursor-pointer" : ""
              }`}
            >
              <div
                className={`mt-0.5 h-5 w-5 shrink-0 rounded-full border flex items-center justify-center ${
                  step.done
                    ? "bg-[var(--v-accent)] border-[var(--v-accent)]"
                    : "border-border bg-background"
                }`}
              >
                {step.done
                  ? <Check size={12} className="text-white" />
                  : <span className="text-[10px] font-mono text-muted-foreground">{i + 1}</span>}
              </div>
              <div className="min-w-0">
                <p className={`text-sm font-medium ${step.done ? "line-through text-muted-foreground" : "text-foreground"}`}>
                  {step.title}
                </p>
                <p className="text-xs text-muted-foreground mt-0.5">{step.description}</p>
              </div>
            </div>
          );

          if (step.done) return <div key={step.title}>{inner}</div>;
          if (step.onClick) return <button type="button" key={step.title} onClick={step.onClick} className="text-left w-full">{inner}</button>;
          if (step.href) return <Link key={step.title} href={step.href}>{inner}</Link>;
          return <div key={step.title}>{inner}</div>;
        })}
      </div>
    </div>
  );
}
