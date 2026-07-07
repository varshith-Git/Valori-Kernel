import { NextRequest, NextResponse } from "next/server";
import { execFileSync } from "child_process";
import { getProject, projectNodePaths, protectProject, touchProject } from "@/lib/server/projects";
import { pm } from "@/lib/server/process-manager";

async function probePort(port: number): Promise<boolean> {
  try {
    const r = await fetch(`http://127.0.0.1:${port}/health`, { signal: AbortSignal.timeout(1500) });
    return r.ok;
  } catch { return false; }
}

function findPidOnPort(port: number): number | null {
  try {
    const out = execFileSync("lsof", ["-ti", `:${port}`], { encoding: "utf8", timeout: 3000 }).trim();
    const pid = parseInt(out.split("\n")[0], 10);
    return isNaN(pid) ? null : pid;
  } catch { return null; }
}

async function snapshotViaHttp(port: number, snapshotPath: string): Promise<void> {
  try {
    await fetch(`http://127.0.0.1:${port}/v1/snapshot/save`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ path: snapshotPath }),
      signal: AbortSignal.timeout(8000),
    });
  } catch { /* WAL is durable regardless */ }
}

// POST — snapshot-on-close: ask every node to write a final snapshot, stop it,
// wait for the process to fully exit, then re-apply the immutable flag so the
// data is protected at rest.
export async function POST(
  _req: NextRequest,
  { params }: { params: Promise<{ name: string }> }
) {
  const { name } = await params;
  const entry = getProject(name);
  if (!entry) {
    return NextResponse.json({ error: `Project "${name}" not found` }, { status: 404 });
  }

  // Capture the final record count before stopping, so the Home card shows
  // accurate info while the project is at rest. Best-effort — cluster nodes
  // have a different /health shape without a records field.
  let finalRecords: number | undefined;
  try {
    const r = await fetch(`http://127.0.0.1:${entry.nodes[0].httpPort}/health`, {
      signal: AbortSignal.timeout(1500),
    });
    if (r.ok) {
      const h = await r.json() as { records?: { live?: number } | number };
      finalRecords = typeof h.records === "number" ? h.records : h.records?.live;
    }
  } catch { /* node may already be down */ }

  await Promise.all(entry.nodes.map(async (n) => {
    const { snapshotPath } = projectNodePaths(entry, n.id);

    // Happy path: PM owns the process and can SIGTERM it directly.
    if (pm.isRunning(n.httpPort)) {
      await pm.snapshotThenStop(n.httpPort, snapshotPath);
      await pm.waitForExit(n.httpPort);
      return;
    }

    // Orphan path: node was started in a previous Next.js session. The PM has
    // no proc handle, so snapshot via HTTP and kill by PID from the port.
    const alive = await probePort(n.httpPort);
    if (!alive) return;

    await snapshotViaHttp(n.httpPort, snapshotPath);

    const pid = findPidOnPort(n.httpPort);
    if (pid) {
      try { process.kill(pid, "SIGTERM"); } catch { /* already dead */ }
      // Give it a moment to flush, then force-kill if still alive.
      await new Promise(r => setTimeout(r, 2000));
      try { process.kill(pid, 0); process.kill(pid, "SIGKILL"); } catch { /* gone */ }
    }
  }));

  // Re-lock the files now that no process is writing them.
  protectProject(name);
  touchProject(name, {
    lastOpenedAt: new Date().toISOString(),
    ...(finalRecords != null ? { records: finalRecords } : {}),
  });

  return NextResponse.json({ ok: true });
}
