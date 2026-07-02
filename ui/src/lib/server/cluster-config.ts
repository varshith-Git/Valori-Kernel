import type { NodeCfg } from "./process-manager";

// Shared helpers for building multi-node Raft cluster configs. Used by both
// the Launcher page's ad-hoc "Cluster" mode and per-project cluster creation
// (replication: 3) — kept here so both flows stay in sync instead of each
// maintaining its own copy.

export function buildMembers(nodes: NodeCfg[], host = "localhost"): string {
  return nodes
    .map(n => `${n.id}=${host}:${n.raftPort ?? (3100 + n.id)}/${host}:${n.httpPort}`)
    .join(",");
}

export function makeDefaultNodes(
  count: number,
  opts: { httpBase?: number; raftBase?: number; dir?: string } = {}
): NodeCfg[] {
  const httpBase = opts.httpBase ?? 3000;
  const raftBase = opts.raftBase ?? 3100;
  const dir      = opts.dir ?? "~/.valori/cluster";
  return Array.from({ length: count }, (_, i) => {
    const id = i + 1;
    return {
      id,
      httpPort:     httpBase + id,
      raftPort:     raftBase + id,
      eventLogPath: `${dir}/n${id}-events.log`,
      snapshotPath: `${dir}/n${id}.snap`,
      raftLogPath:  `${dir}/n${id}-raft.redb`,
      clusterInit:  id === 1,
    };
  });
}

export function nextNodeConfig(existing: NodeCfg[], dir = "~/.valori/cluster"): NodeCfg {
  const maxId   = Math.max(...existing.map(n => n.id));
  const maxHttp = Math.max(...existing.map(n => n.httpPort));
  const maxRaft = Math.max(...existing.map(n => n.raftPort ?? (3100 + n.id)));
  const id = maxId + 1;
  return {
    id,
    httpPort:     maxHttp + 1,
    raftPort:     maxRaft + 1,
    eventLogPath: `${dir}/n${id}-events.log`,
    snapshotPath: `${dir}/n${id}.snap`,
    raftLogPath:  `${dir}/n${id}-raft.redb`,
    clusterInit:  false,
  };
}
