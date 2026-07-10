// TypeScript mirrors of Valori Rust API response shapes.

export interface ProofResponse {
  final_state_hash: string;
  chain_height?: number;
  record_count?: number;
  event_count?: number;
}

export interface SearchResult {
  id: number;
  score: number;
  collection?: string;
  text?: string;
  source?: string;
}

export interface SearchResponse {
  results: SearchResult[];
  state_hash?: string;
  queried_at?: string;
}

export interface Collection {
  name: string;
  id?: number;
  record_count?: number;
}

export interface PoolStats {
  live: number;
  slots_used: number;
  capacity: number;
  fill_pct: number;
}

export interface HealthResponse {
  status: "ok" | "degraded" | "full";
  version?: string;
  dim?: number;
  index?: string;
  records?: PoolStats;
  nodes?: PoolStats;
  edges?: PoolStats;
  event_log_height?: number;
}

export interface ClusterNode {
  node_id: number;
  addr: string;
  role: "leader" | "follower" | "candidate";
  log_index?: number;
  state_hash?: string;
}

export interface ClusterStatus {
  leader_id?: number;
  nodes: ClusterNode[];
  converged: boolean;
}

export interface SearchRequest {
  vector: number[];
  k: number;
  collection?: string;
  consistency?: "local" | "linearizable";
}
