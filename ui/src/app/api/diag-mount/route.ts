import { NextRequest, NextResponse } from "next/server";

// Temporary startup-pipeline diagnostic (Phase D1.3 debugging) — logs to
// stdout, which the desktop shell already captures and prints
// (`ui_server_manager.rs`'s `CommandEvent::Stdout` forwarding). This gives a
// headless-verifiable signal for "did the webview actually load and run JS"
// without needing console-log forwarding or screen access. Remove once the
// blank-window issue is resolved.
export async function GET(req: NextRequest) {
  const stage = req.nextUrl.searchParams.get("stage") ?? "unknown";
  const msg = req.nextUrl.searchParams.get("msg");
  console.log(`[diag-mount] stage=${stage}${msg ? ` msg=${msg}` : ""}`);
  return NextResponse.json({ ok: true, stage });
}
