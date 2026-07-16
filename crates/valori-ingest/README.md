# valori-ingest

Embedding client and document chunking primitives for the Valori platform.

Zero dependency on any other `valori-*` crate — maximally reusable and independently testable.

## Modules

| Module | Responsibility |
|---|---|
| `embed` | Async HTTP embedding client for Ollama, OpenAI, and OpenAI-compatible providers |
| `chunker` | Four deterministic text chunking strategies with auto-detection and BLAKE3 content hashing |
| `handler` | Stateless axum handler for `POST /v1/ingest/document` (chunk-only, no embed) |

## Usage

```rust
use valori_ingest::{EmbedConfig, embed_batch, chunk_document, chunk_content_hash};

// Embed text chunks
let cfg = EmbedConfig {
    provider: "ollama".into(),
    model: "nomic-embed-text".into(),
    url: "http://localhost:11434".into(),
    api_key: None,
};
let http = reqwest::Client::new();
let vectors = embed_batch(&["hello world"], &cfg, &http).await?;

// Chunk a document
let (chunks, strategy_used) = chunk_document(text, "auto", 1000, 200);

// Content-hash for dedup (used by /v1/ingest/update)
let hash = chunk_content_hash(&chunks[0].text);
```

## Chunking strategies

| Strategy | Trigger | How it works |
|---|---|---|
| `tree` | `# Heading` markers ≥ 2 | Splits at markdown headings, preserving section hierarchy |
| `conversation` | `[Name]:` turn lines ≥ 2 | Splits at speaker turns |
| `sentence` | Any prose | Splits at sentence boundaries with configurable window |
| `fixed` | Fallback | Fixed character windows with overlap |
| `auto` | Default | Detects the best strategy from text structure |

Auto-detection order: tree → conversation → sentence → fixed.

If `tree` produces fewer than 2 chunks, it falls back to `fixed` to avoid recursion.

## Embedding providers

| Provider | Default model | Default URL |
|---|---|---|
| `ollama` | `nomic-embed-text` | `http://localhost:11434` |
| `openai` | `text-embedding-3-small` | `https://api.openai.com` |
| `custom` | *(required)* | *(required)* |

## Invariants

- `chunk_content_hash` is deterministic: same text → same 32-byte BLAKE3 hash
- `MAX_INGEST_TEXT_BYTES = 10 MiB` — texts larger than this are rejected at the handler level
- Chunking is pure and synchronous; embedding is async and network-dependent
- `ingest_document` is stateless (no `State<>`) and compiles unchanged into both standalone and cluster routers

## Design (SOLID)

- **SRP**: each module owns exactly one concern (embed client, chunker, stateless handler)
- **OCP**: new embedding providers add a match arm in `embed_batch`, no structural change
- **ISP**: callers that only chunk import only `chunker`; callers that only embed import only `embed`
- **DIP**: `embed_batch` takes `&reqwest::Client` (injected), not a created-internally client
