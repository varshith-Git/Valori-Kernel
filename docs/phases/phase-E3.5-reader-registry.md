# Phase E3.5 — ReaderRegistry

## Goal

Centralise all extension-to-reader mapping in a single `ReaderRegistry` inside `valori-ingest`. The pipeline, node, daemon, and CLI must never contain extension-matching logic.

## Delivered

| File | Contents |
|---|---|
| `crates/valori-ingest/src/registry.rs` | `ReaderRegistry` — `reader_for_extension(ext)` + `reader_for_path(path)`, returns `Arc<dyn Reader>` |
| `crates/valori-ingest/src/lib.rs` | Added `pub mod registry` + `pub use registry::ReaderRegistry` |

## API

```rust
// by extension (no leading dot, case-insensitive)
let reader = ReaderRegistry::reader_for_extension("md")?;

// by file path (extension extracted automatically)
let reader = ReaderRegistry::reader_for_path("/docs/report.pdf")?;

// both return Arc<dyn Reader> — cheap to clone, Send + Sync
reader.read(input, Some("report.pdf")).await?;
```

Supported extensions: `txt`, `text`, `md`, `markdown`, `html`, `htm`, `pdf`, `docx`.
Unknown extension → `IngestError::Reader("no reader registered for extension '.xyz'")`.

## Findings

- `Arc<dyn Reader>` does not implement `Debug`, so `Result::unwrap_err()` is not callable on `Result<Arc<dyn Reader>, _>` in tests. Used `match` to extract the error value directly.

## Validation

```
cargo test -p valori-ingest  → 49 passed, 0 failed (was 34 before E3.5)
cargo build -p valori-node   → 0 errors
```

15 new registry tests:
- One test per supported extension (`txt`, `text`, `md`, `markdown`, `html`, `htm`, `pdf`, `docx`)
- Case-insensitive dispatch (`MD`, `PDF`, `DOCX`, `HTML`)
- Unknown extension error with message content
- Empty extension error
- Path dispatch for all five base formats
- Path with directory component
- Path without extension (no dot)
- Path with unknown extension

## Follow-ups

- Mime-type dispatch (`reader_for_mime("text/markdown")`) — straightforward to add when a caller has a MIME type instead of a path.
- `reader_for_path` could fall back to content sniffing for files without extensions.
