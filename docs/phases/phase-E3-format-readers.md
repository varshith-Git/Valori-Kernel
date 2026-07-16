# Phase E3 — Format Readers (Markdown, HTML, PDF, DOCX)

## Goal

Implement four `Reader` implementations inside `valori-ingest` so the pipeline can consume text, Markdown, HTML, PDF, and DOCX without changing any pipeline code.

## Delivered

| File | Contents |
|---|---|
| `crates/valori-ingest/src/readers/markdown.rs` | `MarkdownReader` — strips CommonMark formatting via `pulldown-cmark`; extracts H1 title into metadata |
| `crates/valori-ingest/src/readers/html.rs` | `HtmlReader` — visible-text extraction via `scraper`; prunes `<script>`/`<style>` subtrees; extracts `<title>` and `<meta name="author">` |
| `crates/valori-ingest/src/readers/pdf.rs` | `PdfReader` — file-path input; text via `pdf-extract`; page count via `lopdf`; async via `spawn_blocking` |
| `crates/valori-ingest/src/readers/docx.rs` | `DocxReader` — file-path input; opens ZIP, parses `word/document.xml` for `<w:t>` runs and `docProps/core.xml` for title/author via `quick-xml`; async via `spawn_blocking` |
| `crates/valori-ingest/src/readers/mod.rs` | Module root + re-exports |
| `crates/valori-ingest/src/lib.rs` | Added `pub mod readers` + re-exports for all four readers |
| `crates/valori-ingest/Cargo.toml` | Added `pulldown-cmark`, `scraper`, `pdf-extract`, `lopdf`, `zip`, `quick-xml`, `tokio` |

## Findings

- `scraper`'s `.descendants()` walker visits all descendants including script/style text nodes. Fixed by switching to a recursive `collect_text()` that prunes subtrees via `skip.matches()` before descending.
- `lopdf` must be declared as a direct dep (not just transitive via `pdf-extract`) to call `Document::load()` directly for page-count extraction.
- `tokio` promoted from dev-dep to regular dep because PDF and DOCX readers use `spawn_blocking` in non-test code.

## Validation

```
cargo test -p valori-ingest  → 34 passed, 0 failed (was 19 before E3)
cargo build -p valori-node   → 0 errors
```

Tests by reader:
- `MarkdownReader`: 5 tests (plain text extraction, H1 title, no-title, default source, ID stability)
- `HtmlReader`: 4 tests (visible text, title+author, MIME type, default source)
- `PdfReader`: 2 tests (missing file error, MIME constant — file-round-trip requires fixture)
- `DocxReader`: 4 tests (missing file error, MIME constant, XML text run parser, core props parser)

## Follow-ups

- Integration tests with real fixture files (PDF + DOCX) for round-trip coverage — omitted here to keep the crate free of binary blobs.
- `PdfReader` and `DocxReader` take file paths; a future `BytesReader` wrapper could accept `&[u8]` for callers that already have file bytes in memory.
- E4: wire readers into the pipeline via a `format_reader(path)` factory that selects the right reader from file extension.
