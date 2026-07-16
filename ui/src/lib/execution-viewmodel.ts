// Adapter: valori-node's `ExecutionRecord`/`StageView` (the real backend DTO,
// see crates/valori-node/src/execution_registry.rs) → a UI-shaped view model.
// Every visual piece (timeline, DAG, panels) consumes ONLY this — never the
// raw backend shape — so a backend field rename never means a component
// rewrite, just an edit here.

export interface StageViewModel {
  /** Stable key, e.g. "embedder" — used for React keys and node ids, never shown. */
  id: string;
  /** Human-facing name, e.g. "Generate embeddings". */
  title: string;
  durationMs: number;
  startedAtMs: number;
  success: boolean;
  warnings: string[];
  error?: string | null;
  /** Stage-specific counters, already flattened to primitives — e.g.
   *  `{ provider: "ollama", model: "all-minilm", batch_count: 1 }` for the
   *  embed stage, `{ records_written: 3, graph_nodes_created: 3 }` for write. */
  metrics: Record<string, string | number | boolean>;
}

export interface ExecutionViewModel {
  operationId: string;
  documentSource: string;
  collection: string;
  stages: StageViewModel[];
  chunksProduced: number;
  recordsWritten: number;
  totalDurationMs: number;
  success: boolean;
  error?: string | null;
  receiptId?: string | null;
  stateHashBefore?: string | null;
  stateHashAfter?: string | null;
}

// ── Raw backend shape (crate valori-node's ExecutionRecord/StageView) ───────

interface RawStageView {
  label: string;
  stage: string;
  started_at_ms: number;
  duration_ms: number;
  success: boolean;
  warnings: string[];
  metrics: Record<string, unknown>;
  error: string | null;
}

interface RawExecutionRecord {
  operation_id: string;
  document_source: string;
  collection: string;
  stages: RawStageView[];
  chunks_produced: number;
  records_written: number;
  total_duration_ms: number;
  success: boolean;
  error: string | null;
  receipt_id?: string | null;
  state_hash_before?: string | null;
  state_hash_after?: string | null;
}

function flattenMetrics(metrics: Record<string, unknown>): Record<string, string | number | boolean> {
  const out: Record<string, string | number | boolean> = {};
  for (const [key, value] of Object.entries(metrics)) {
    // `stage` is the serde-tag discriminant (StageMetrics is a tagged enum) —
    // internal wiring, not something a user should see.
    if (key === "stage") continue;
    if (typeof value === "string" || typeof value === "number" || typeof value === "boolean") {
      out[key] = value;
    } else if (Array.isArray(value)) {
      out[key] = value.length;
    }
  }
  return out;
}

/** Type guard: does `data` look like a real `ExecutionRecord` (vs. the old
 *  mock `ExecutionGraph` shape, or an error body)? Lets callers detect a
 *  stale cached response without crashing. */
export function isExecutionRecord(data: unknown): data is RawExecutionRecord {
  return (
    !!data &&
    typeof data === "object" &&
    "operation_id" in data &&
    "stages" in data &&
    Array.isArray((data as { stages: unknown }).stages)
  );
}

export function toExecutionViewModel(raw: RawExecutionRecord): ExecutionViewModel {
  return {
    operationId: raw.operation_id,
    documentSource: raw.document_source,
    collection: raw.collection,
    stages: raw.stages.map((s) => ({
      id: s.stage,
      title: s.label,
      durationMs: s.duration_ms,
      startedAtMs: s.started_at_ms,
      success: s.success,
      warnings: s.warnings ?? [],
      error: s.error,
      metrics: flattenMetrics(s.metrics ?? {}),
    })),
    chunksProduced: raw.chunks_produced,
    recordsWritten: raw.records_written,
    totalDurationMs: raw.total_duration_ms,
    success: raw.success,
    error: raw.error,
    receiptId: raw.receipt_id ?? null,
    stateHashBefore: raw.state_hash_before ?? null,
    stateHashAfter: raw.state_hash_after ?? null,
  };
}

/** Present a metric key as a label, e.g. "graph_nodes_created" → "Graph nodes created". */
export function metricLabel(key: string): string {
  return key.charAt(0).toUpperCase() + key.slice(1).replace(/_/g, " ");
}
