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
