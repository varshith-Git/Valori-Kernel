import type {
  ProofResponse,
  SearchRequest,
  SearchResponse,
  Collection,
  HealthResponse,
  ClusterStatus,
} from "@/types/valori";

export interface IValoriClient {
  getProof(): Promise<ProofResponse>;
  getHealth(): Promise<HealthResponse>;
  search(req: SearchRequest): Promise<SearchResponse>;
  listCollections(): Promise<Collection[]>;
  createCollection(name: string): Promise<void>;
  dropCollection(name: string): Promise<void>;
  getClusterStatus(): Promise<ClusterStatus>;
}

export class ValoriClient implements IValoriClient {
  private readonly base: string;
  private readonly token: string | undefined;

  constructor(base: string, token?: string) {
    this.base = base.replace(/\/$/, "");
    this.token = token;
  }

  private headers(): HeadersInit {
    const h: Record<string, string> = { "Content-Type": "application/json" };
    if (this.token) h["Authorization"] = `Bearer ${this.token}`;
    return h;
  }

  private async get<T>(path: string): Promise<T> {
    const res = await fetch(`${this.base}${path}`, {
      headers: this.headers(),
      cache: "no-store",
    });
    if (!res.ok) throw new Error(`GET ${path} → ${res.status}`);
    return res.json();
  }

  private async post<T>(path: string, body?: unknown): Promise<T> {
    const res = await fetch(`${this.base}${path}`, {
      method: "POST",
      headers: this.headers(),
      body: body != null ? JSON.stringify(body) : undefined,
    });
    if (!res.ok) throw new Error(`POST ${path} → ${res.status}`);
    return res.json();
  }

  private async delete(path: string): Promise<void> {
    const res = await fetch(`${this.base}${path}`, {
      method: "DELETE",
      headers: this.headers(),
    });
    if (!res.ok) throw new Error(`DELETE ${path} → ${res.status}`);
  }

  async getProof(): Promise<ProofResponse> {
    const raw = await this.get<{ final_state_hash: string }>("/v1/proof/state");
    return raw;
  }

  async getHealth(): Promise<HealthResponse> {
    return this.get<HealthResponse>("/health");
  }

  async search(req: SearchRequest): Promise<SearchResponse> {
    const payload = {
      query: req.vector,   // Valori REST API field name is "query"
      k: req.k,
      ...(req.collection ? { collection: req.collection } : {}),
      ...(req.consistency ? { consistency: req.consistency } : {}),
    };
    const raw = await this.post<{ results: Array<{ id: number; score: number }> }>(
      "/search",
      payload
    );
    return {
      results: raw.results,
      queried_at: new Date().toISOString(),
    };
  }

  async listCollections(): Promise<Collection[]> {
    const raw = await this.get<
      { collections: Array<{ name: string; id?: number; record_count?: number }> } | string[]
    >("/v1/namespaces");
    if (Array.isArray(raw)) {
      return raw.map((name) => ({ name }));
    }
    return raw.collections ?? [];
  }

  async createCollection(name: string): Promise<void> {
    await this.post("/v1/namespaces", { name });
  }

  async dropCollection(name: string): Promise<void> {
    await this.delete(`/v1/namespaces/${encodeURIComponent(name)}`);
  }

  async getClusterStatus(): Promise<ClusterStatus> {
    const raw = await this.get<{
      leader_id?: number;
      members?: Array<{ node_id: number; addr: string; role: string; log_index?: number }>;
    }>("/v1/cluster/status");
    return {
      leader_id: raw.leader_id,
      nodes: (raw.members ?? []).map((m) => ({
        node_id: m.node_id,
        addr: m.addr,
        role: m.role as "leader" | "follower" | "candidate",
        log_index: m.log_index,
      })),
      converged: true,
    };
  }
}
