// TypeScript mirrors of Valori Rust API response shapes.

export interface ProofResponse {
  final_state_hash: string;
  chain_height?: number;
  record_count?: number;
  event_count?: number;
}

export interface SearchResult {
  id: string;
  distance: number;
  collection?: string;
}

export interface SearchResponse {
  results: SearchResult[];
  state_hash?: string;
  queried_at?: string;
}

export interface Collection {
  name: string;
  record_count?: number;
}

export interface HealthResponse {
  status: "ok" | "degraded" | "error";
  node_id?: number;
  role?: "leader" | "follower" | "candidate";
  record_count?: number;
  pool_used?: number;
  pool_cap?: number;
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
