"use client";

import { useEffect, useState } from "react";
import Image from "next/image";
import { Globe, Code2, CheckCircle2 } from "lucide-react";
import { createClient } from "@/utils/supabase/client";

interface Props {
  onSignedIn: () => void;
}

export function SignInGate({ onSignedIn }: Props) {
  const [email, setEmail] = useState<string | null>(null);

  // Check for an existing session on mount (e.g. user re-ran setup after
  // already signing in, or navigated back here).
  useEffect(() => {
    if (!process.env.NEXT_PUBLIC_SUPABASE_URL) return;
    const supabase = createClient();
    supabase.auth.getSession().then(({ data }) => {
      if (data.session?.user?.email) setEmail(data.session.user.email);
    });
  }, []);

  // Navigate the webview to the local login page so OAuth runs in-process.
  // This avoids the deep-link handoff (valori://auth-callback) which requires
  // the app to be registered as a URL scheme handler — unreliable in dev mode
  // and on first install before macOS has indexed the bundle. After OAuth
  // the callback route sets the session in cookies and redirects to /, where
  // AppShellGate will find the session and skip this gate.
  const handleSignIn = (provider: "google" | "github") => {
    window.location.href = `/login?next=/&provider=${provider}`;
  };

  const btnStyle: React.CSSProperties = {
    width: "100%",
    maxWidth: 320,
    display: "flex",
    alignItems: "center",
    gap: 12,
    padding: "13px 20px",
    borderRadius: 10,
    border: "1px solid var(--border)",
    background: "var(--card)",
    color: "var(--foreground)",
    fontSize: 14,
    fontWeight: 500,
    cursor: "pointer",
    transition: "border-color 0.15s, background 0.15s",
    textAlign: "left" as const,
  };

  return (
    <div
      style={{
        position: "fixed",
        inset: 0,
        display: "flex",
        flexDirection: "column",
        alignItems: "center",
        justifyContent: "center",
        background: "var(--background)",
        gap: 0,
        fontFamily: "-apple-system, BlinkMacSystemFont, 'Segoe UI', sans-serif",
      }}
    >
      {/* Logo + brand */}
      <div style={{ display: "flex", alignItems: "center", gap: 10, marginBottom: 32 }}>
        <div
          style={{
            width: 40,
            height: 40,
            borderRadius: 10,
            background: "var(--v-accent-muted)",
            border: "1px solid color-mix(in oklch, var(--v-accent) 30%, transparent)",
            display: "flex",
            alignItems: "center",
            justifyContent: "center",
          }}
        >
          <Image src="/logo.png" alt="Valori" width={24} height={24} className="dark:invert" style={{ height: "auto" }} />
        </div>
        <span style={{ fontSize: 18, fontWeight: 700, color: "var(--foreground)", letterSpacing: "-0.3px" }}>
          valori
        </span>
      </div>

      {email ? (
        /* ── Success state ── */
        <div
          style={{
            display: "flex",
            flexDirection: "column",
            alignItems: "center",
            gap: 16,
            marginBottom: 32,
          }}
        >
          <div
            style={{
              width: 64,
              height: 64,
              borderRadius: "50%",
              background: "color-mix(in oklch, var(--v-accent) 12%, transparent)",
              border: "1.5px solid color-mix(in oklch, var(--v-accent) 40%, transparent)",
              display: "flex",
              alignItems: "center",
              justifyContent: "center",
            }}
          >
            <CheckCircle2 size={30} color="var(--v-accent)" />
          </div>
          <div style={{ textAlign: "center" }}>
            <p style={{ fontSize: 20, fontWeight: 600, color: "var(--foreground)", margin: "0 0 6px" }}>
              You&apos;re signed in!
            </p>
            <p style={{ fontSize: 13, color: "var(--muted-foreground)", margin: 0 }}>{email}</p>
          </div>
          <button
            style={{
              marginTop: 8,
              padding: "11px 32px",
              borderRadius: 10,
              border: "none",
              background: "var(--v-accent)",
              color: "#fff",
              fontSize: 14,
              fontWeight: 600,
              cursor: "pointer",
            }}
            onClick={onSignedIn}
          >
            Continue to Valori
          </button>
        </div>
      ) : (
        /* ── Sign-in buttons ── */
        <>
          <div style={{ textAlign: "center", marginBottom: 32 }}>
            <p style={{ fontSize: 22, fontWeight: 600, color: "var(--foreground)", margin: "0 0 8px" }}>
              Sign in to continue
            </p>
            <p style={{ fontSize: 13, color: "var(--muted-foreground)", margin: 0, lineHeight: 1.6 }}>
              Create or sign in to your Valori account to get started.
            </p>
          </div>

          <div style={{ display: "flex", flexDirection: "column", gap: 10, width: "100%", maxWidth: 320 }}>
            <button
              style={btnStyle}
              onMouseEnter={(e) => {
                (e.currentTarget as HTMLButtonElement).style.borderColor = "var(--v-accent)";
                (e.currentTarget as HTMLButtonElement).style.background = "var(--v-accent-muted)";
              }}
              onMouseLeave={(e) => {
                (e.currentTarget as HTMLButtonElement).style.borderColor = "var(--border)";
                (e.currentTarget as HTMLButtonElement).style.background = "var(--card)";
              }}
              onClick={() => handleSignIn("google")}
            >
              <Globe size={16} style={{ color: "var(--muted-foreground)", flexShrink: 0 }} />
              Continue with Google
            </button>

            <button
              style={btnStyle}
              onMouseEnter={(e) => {
                (e.currentTarget as HTMLButtonElement).style.borderColor = "var(--v-accent)";
                (e.currentTarget as HTMLButtonElement).style.background = "var(--v-accent-muted)";
              }}
              onMouseLeave={(e) => {
                (e.currentTarget as HTMLButtonElement).style.borderColor = "var(--border)";
                (e.currentTarget as HTMLButtonElement).style.background = "var(--card)";
              }}
              onClick={() => handleSignIn("github")}
            >
              <Code2 size={16} style={{ color: "var(--muted-foreground)", flexShrink: 0 }} />
              Continue with GitHub
            </button>
          </div>

          {/* Dev-only bypass — never rendered in a production build, so this
              can't ship as a real auth hole. Lets local development proceed
              without a real OAuth round-trip through Supabase. */}
          {process.env.NODE_ENV === "development" && (
            <button
              style={{
                marginTop: 20,
                background: "none",
                border: "none",
                color: "var(--muted-foreground)",
                fontSize: 12,
                cursor: "pointer",
                textDecoration: "underline",
                textUnderlineOffset: 3,
              }}
              onClick={onSignedIn}
            >
              Skip sign-in (dev only)
            </button>
          )}
        </>
      )}

      <p
        style={{
          position: "absolute",
          bottom: 24,
          fontSize: 11,
          color: "var(--muted-foreground)",
          textAlign: "center",
          margin: 0,
        }}
      >
        Your data stays on this machine unless you create a cloud project.
      </p>

    </div>
  );
}
