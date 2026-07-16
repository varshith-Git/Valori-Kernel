"use client";

import { Component, type ReactNode } from "react";
import { AlertTriangle, RotateCcw, Copy, Check } from "lucide-react";

interface Props {
  children: ReactNode;
}

interface State {
  error: Error | null;
  copied: boolean;
}

export class GlobalErrorBoundary extends Component<Props, State> {
  state: State = { error: null, copied: false };

  static getDerivedStateFromError(error: Error): Partial<State> {
    return { error };
  }

  componentDidCatch(error: Error, info: { componentStack: string }) {
    console.error("[GlobalErrorBoundary]", error, info.componentStack);
  }

  private details(): string {
    const { error } = this.state;
    if (!error) return "";
    return [
      `Error: ${error.message}`,
      error.stack ?? "",
    ].join("\n");
  }

  private handleCopy = async () => {
    try {
      await navigator.clipboard.writeText(this.details());
      this.setState({ copied: true });
      setTimeout(() => this.setState({ copied: false }), 2000);
    } catch {
      /* clipboard denied */
    }
  };

  render() {
    if (!this.state.error) return this.props.children;

    const { error, copied } = this.state;

    return (
      <div className="flex h-full w-full items-center justify-center bg-background p-8">
        <div className="w-full max-w-lg rounded-2xl border border-destructive/30 bg-card shadow-xl">
          {/* Header */}
          <div className="flex items-start gap-3 px-6 pt-6">
            <div className="flex h-9 w-9 shrink-0 items-center justify-center rounded-xl bg-destructive/10 border border-destructive/20">
              <AlertTriangle size={16} className="text-destructive" />
            </div>
            <div>
              <h1 className="text-sm font-semibold text-foreground">Something went wrong</h1>
              <p className="mt-0.5 text-xs text-muted-foreground">
                A rendering error occurred. Your data is safe — reload to continue.
              </p>
            </div>
          </div>

          {/* Error message */}
          <div className="mx-6 mt-4 rounded-lg border border-border bg-background px-4 py-3">
            <p className="font-mono text-xs text-destructive break-all leading-relaxed">
              {error.message || "Unknown error"}
            </p>
          </div>

          {/* Actions */}
          <div className="flex items-center gap-2 border-t border-border/60 px-6 py-4 mt-4">
            <button
              onClick={() => window.location.reload()}
              className="flex items-center gap-1.5 rounded-lg bg-primary px-4 py-2 text-xs font-medium text-primary-foreground hover:bg-primary/90 transition-colors"
            >
              <RotateCcw size={12} />
              Reload app
            </button>
            <button
              onClick={this.handleCopy}
              className="flex items-center gap-1.5 rounded-lg border border-border bg-background px-4 py-2 text-xs font-medium text-foreground hover:bg-accent transition-colors"
            >
              {copied ? <Check size={12} className="text-emerald-500" /> : <Copy size={12} />}
              {copied ? "Copied" : "Copy error details"}
            </button>
          </div>
        </div>
      </div>
    );
  }
}
