"use client";

import { useEffect, useState } from "react";
import { useRouter } from "next/navigation";
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Button } from "@/components/ui/button";
import { HardDrive, Cloud, LogIn, Loader2 } from "lucide-react";
import { createClient } from "@/utils/supabase/client";

interface Props {
  open: boolean;
  onClose: () => void;
  /** Chosen "Local" — caller opens the existing (unchanged) local CreateProjectDialog. */
  onLocal: () => void;
}

type SignInState = "checking" | "signed-out" | "signed-in" | "unavailable";

/**
 * First step of "New Project": local (this laptop's RAM/disk, the daemon
 * you already have) vs cloud (a real internet-hosted node via Valori
 * Cloud). Cloud requires a Supabase session — outside the desktop shell
 * there's no concept of "not signed in yet" (the website IS always
 * signed-in-or-redirected-to-login), so this picker is really a desktop
 * concern, but the component doesn't hard-fail in a browser tab either.
 *
 * Cloud project creation itself lives at /cloud (ported from valori-ui) —
 * signed-in users are sent straight there instead of creating inline here.
 */
export function ProjectModePicker({ open, onClose, onLocal }: Props) {
  const router = useRouter();
  const [signIn, setSignIn] = useState<SignInState>("checking");

  useEffect(() => {
    if (!open) return;

    // A build with no cloud credentials configured (the normal state for a
    // local-only dev/CI environment) has no Supabase project to check a
    // session against at all — createClient() throws immediately if asked
    // to, rather than just returning null. Treat that as its own state
    // instead of a crash.
    if (!process.env.NEXT_PUBLIC_SUPABASE_URL) {
      setSignIn("unavailable");
      return;
    }

    setSignIn("checking");
    const supabase = createClient();
    supabase.auth.getSession().then(({ data: { session } }) => {
      setSignIn(session ? "signed-in" : "signed-out");
    });
  }, [open]);

  return (
    <Dialog open={open} onOpenChange={(o) => !o && onClose()}>
      <DialogContent className="bg-card border-input max-w-lg p-0">
        <div className="px-5 pt-5 pb-4 border-b border-border">
          <DialogHeader>
            <DialogTitle className="text-foreground text-base font-semibold">New Project</DialogTitle>
            <p className="text-[11px] text-muted-foreground mt-0.5">
              Where should this project's data live?
            </p>
          </DialogHeader>
        </div>

        <div className="grid grid-cols-2 gap-3 p-5">
          <button
            type="button"
            onClick={() => { onClose(); onLocal(); }}
            className="flex flex-col gap-2 rounded-lg border border-input bg-background p-4 text-left transition-colors hover:border-[var(--v-accent)] hover:bg-[var(--v-accent-muted)]"
          >
            <HardDrive size={18} className="text-muted-foreground" />
            <div>
              <p className="text-sm font-medium text-foreground">Local</p>
              <p className="text-[11px] text-muted-foreground mt-0.5">
                Runs on this machine — this laptop's RAM and disk, no account needed.
              </p>
            </div>
          </button>

          <div className="flex flex-col gap-2 rounded-lg border border-input bg-background p-4">
            <Cloud size={18} className="text-muted-foreground" />
            <div className="flex-1">
              <p className="text-sm font-medium text-foreground">Cloud</p>
              <p className="text-[11px] text-muted-foreground mt-0.5">
                A real node on the internet, managed by Valori Cloud.
              </p>
            </div>

            {signIn === "checking" && (
              <div className="flex items-center gap-1.5 text-[11px] text-muted-foreground">
                <Loader2 size={12} className="animate-spin" /> Checking sign-in…
              </div>
            )}

            {signIn === "signed-out" && (
              <Button
                size="sm"
                variant="outline"
                className="w-full text-xs"
                onClick={() => { onClose(); router.push("/login?next=/cloud"); }}
              >
                <LogIn size={13} className="mr-1.5" /> Sign in to sync
              </Button>
            )}

            {signIn === "signed-in" && (
              <Button
                size="sm"
                variant="outline"
                className="w-full text-xs"
                onClick={() => { onClose(); router.push("/cloud"); }}
              >
                <Cloud size={13} className="mr-1.5" /> Go to Valori Cloud
              </Button>
            )}

            {signIn === "unavailable" && (
              <p className="text-[11px] text-muted-foreground">
                Not configured in this build.
              </p>
            )}
          </div>
        </div>
      </DialogContent>
    </Dialog>
  );
}
