import { NextRequest, NextResponse } from "next/server";
import { pm, LaunchConfig } from "@/lib/server/process-manager";

interface JoinRequest {
  config:           LaunchConfig; // full config including the new node at newNodeIdx
  newNodeIdx:       number;
  anyRunningPort:   number;       // HTTP port of any already-running node
}

interface ClusterMember {
  node_id:   number;
  raft_addr: string;
  api_addr:  string;
  state:     string;
}

interface ClusterStatus {
  current_leader: number | null;
  members: ClusterMember[];
}

async function findLeaderUrl(anyPort: number): Promise<string> {
  const base = `http://localhost:${anyPort}`;
  const r = await fetch(`${base}/v1/cluster/status`, { signal: AbortSignal.timeout(5000) });
  if (!r.ok) throw new Error(`Cluster status ${r.status} from :${anyPort}`);
  const s = await r.json() as ClusterStatus;
  if (s.current_leader == null) throw new Error("No leader elected yet");
  const leader = s.members.find(m => m.node_id === s.current_leader);
  if (!leader) throw new Error(`Leader ${s.current_leader} not found in member list`);
  // api_addr is "host:port"; prepend http://
  return `http://${leader.api_addr}`;
}

async function waitForNode(httpPort: number, maxMs = 30_000): Promise<void> {
  const deadline = Date.now() + maxMs;
  while (Date.now() < deadline) {
    try {
      const r = await fetch(`http://localhost:${httpPort}/health`, {
        signal: AbortSignal.timeout(1000),
      });
      if (r.ok) return;
    } catch {}
    await new Promise(res => setTimeout(res, 500));
  }
  throw new Error(`Node :${httpPort} did not become healthy within ${maxMs / 1000}s`);
}

export async function POST(req: NextRequest) {
  let body: JoinRequest;
  try {
    body = await req.json() as JoinRequest;
  } catch {
    return NextResponse.json({ error: "invalid JSON" }, { status: 400 });
  }
  const { config, newNodeIdx, anyRunningPort } = body;
  const nc = config.nodes[newNodeIdx];

  try {
    // 1. Start the new node process
    pm.startNode(config, newNodeIdx);

    // 2. Wait for it to be reachable
    await waitForNode(nc.httpPort);

    // 3. Find the current Raft leader
    const leaderUrl = await findLeaderUrl(anyRunningPort);

    // 4. Register with the leader (learner → voter)
    const joinRes = await fetch(`${leaderUrl}/v1/cluster/add-node`, {
      method:  "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        node_id:   nc.id,
        raft_addr: `localhost:${nc.raftPort ?? (3100 + nc.id)}`,
        api_addr:  `localhost:${nc.httpPort}`,
      }),
      signal: AbortSignal.timeout(15_000),
    });

    if (!joinRes.ok) {
      const err = await joinRes.text();
      return NextResponse.json(
        { error: `Leader rejected add-node: ${joinRes.status} — ${err}` },
        { status: 502 }
      );
    }

    return NextResponse.json({ ok: true, node_id: nc.id });
  } catch (e) {
    return NextResponse.json({ error: String(e) }, { status: 500 });
  }
}
