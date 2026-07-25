// Canonical vector-dimension option list, shared by CreateProjectDialog and the
// Launcher page. Union of the two lists that used to be maintained separately.

export interface DimensionOption {
  value: number;
  label: string;
}

export const DIMENSIONS: DimensionOption[] = [
  { value: 128,  label: "128  — tiny / tests" },
  { value: 256,  label: "256  — lightweight" },
  { value: 384,  label: "384  — MiniLM-L6-v2, paraphrase-MiniLM" },
  { value: 512,  label: "512  — CLIP ViT-B/32" },
  { value: 768,  label: "768  — BERT-base, all-mpnet-base-v2, nomic" },
  { value: 1024, label: "1024 — BERT-large, bge-large-en" },
  { value: 1536, label: "1536 — text-embedding-ada-002, e5-large" },
  { value: 2048, label: "2048 — e5-mistral-7b" },
  { value: 3072, label: "3072 — text-embedding-3-large" },
  { value: 4096, label: "4096 — Llama / Mistral hidden-state" },
];

export const DEFAULT_DIMENSION = 768;

// Cloud CreateProjectDialog's index-type picker (app/cloud/CreateProjectDialog.tsx)
// — the local one keeps its own inline INDEX_META instead, unaffected by this.
export type IndexType = "brute" | "hnsw" | "ivf" | "bq" | "auto";

export const INDEX_TYPES: { value: IndexType; label: string; title: string }[] = [
  { value: "auto",  label: "Auto",  title: "Auto: brute-force < 10k · BQ 10k–2M · HNSW > 2M" },
  { value: "brute", label: "Brute", title: "Exact nearest neighbor — no approximation" },
  { value: "hnsw",  label: "HNSW",  title: "Graph-based ANN — best for large, high-recall workloads" },
  { value: "ivf",   label: "IVF",   title: "Inverted file index — clusters vectors for faster search" },
  { value: "bq",    label: "BQ",    title: "Binary quantization — compact, fast, mid-size collections" },
];

export const DEFAULT_INDEX_TYPE = "brute" as const;
