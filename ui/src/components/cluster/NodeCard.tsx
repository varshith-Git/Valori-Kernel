import type { MemberView } from "@/lib/hooks/useCluster";

interface Props {
  member: MemberView;
  isLeader: boolean;
  isThisNode: boolean;
}

export function NodeCard({ member, isLeader, isThisNode }: Props) {
  return (
    <div
      className={`rounded-xl border p-5 flex flex-col gap-3 transition-colors ${
        isLeader
          ? "border-emerald-800 bg-emerald-950/30"
          : "border-zinc-800 bg-zinc-900"
      }`}
    >
      {/* Header row */}
      <div className="flex items-start justify-between">
        <div className="flex items-center gap-2">
          <span className="font-mono text-lg font-semibold text-white">
            node-{member.id}
          </span>
          {isThisNode && (
            <span className="text-xs rounded px-1.5 py-0.5 bg-zinc-800 text-zinc-400 border border-zinc-700">
              this node
            </span>
          )}
        </div>
        <div className="flex gap-1.5">
          {isLeader && (
            <span className="text-xs rounded px-2 py-0.5 bg-emerald-900 text-emerald-300 border border-emerald-800 font-medium">
              LEADER
            </span>
          )}
          {member.voter ? (
            <span className="text-xs rounded px-2 py-0.5 bg-zinc-800 text-zinc-400 border border-zinc-700">
              voter
            </span>
          ) : (
            <span className="text-xs rounded px-2 py-0.5 bg-amber-950 text-amber-400 border border-amber-900">
              learner
            </span>
          )}
        </div>
      </div>

      {/* Addresses */}
      <div className="flex flex-col gap-1">
        <div className="flex items-center gap-2">
          <span className="text-[10px] uppercase tracking-widest text-zinc-600 w-10">API</span>
          <code className="text-xs font-mono text-zinc-300">{member.api_addr}</code>
        </div>
        <div className="flex items-center gap-2">
          <span className="text-[10px] uppercase tracking-widest text-zinc-600 w-10">Raft</span>
          <code className="text-xs font-mono text-zinc-500">{member.raft_addr}</code>
        </div>
      </div>
    </div>
  );
}
