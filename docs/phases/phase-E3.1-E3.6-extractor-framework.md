# Phases E3.1–E3.6 — Extractor Framework

## Goal

Harden the `valori-ingest` architecture by separating I/O from parsing, adding typed document metadata, structural validation, MIME-based dispatch, a capabilities API, and a typed source enum. No pipeline changes, no HTTP changes, no daemon changes.

## Delivered

| File | Phase | Purpose |
|---|---|---|
| `src/metadata.rs` | E3.2 | `DocumentMetadata` — typed title/author/language/created_at/modified_at/page_count |
| `src/extractor.rs` | E3.1/E3.5 | `Extractor` trait (bytes-in/Document-out) + `ReaderCapabilities` struct |
| `src/extractors/{text,markdown,html,pdf,docx}.rs` | E3.1 | Five `Extractor` implementations reusing reader parsing helpers |
| `src/extractors/mod.rs` | E3.1 | Module root + re-exports |
| `src/extractor_registry.rs` | E3.1/E3.4/E3.5 | `ExtractorRegistry`: `extractor_for_extension`, `extractor_for_mime`, `extractor_for_path`, `extractor_for_bytes` (MIME detection), `all_capabilities()` |
| `src/validator.rs` | E3.3 | `ValidationError` + `DocumentValidator` (empty, size, pages, UTF-8, protected PDF) + `validate_utf8()` |
| `src/source.rs` | E3.6 | `DocumentSource` enum: `File`, `Url`, `Memory`, `GitHub`, `S3` + `as_source_str()` + `From<&str>` |
| `src/document.rs` | E3.2 | `metadata: Value` → `metadata: DocumentMetadata` on `Document`; `Chunk.metadata` stays `Value` |
| `src/readers/markdown.rs` | E3.2 | Uses `DocumentMetadata`; `extract_text_and_title` made `pub` |
| `src/readers/html.rs` | E3.2 | Uses `DocumentMetadata`; `extract_meta`, `extract_meta_name`, `extract_body_text` made `pub` |
| `src/readers/pdf.rs` | E3.2 | Uses `DocumentMetadata` |
| `src/readers/docx.rs` | E3.2 | Uses `DocumentMetadata`; `extract_text_runs`, `extract_core_props` made `pub` |
| `src/reader.rs` (TextReader) | E3.2 | Uses `DocumentMetadata::default()` |
| `Cargo.toml` | E3.4 | Added `infer = "0.16"` for magic-byte MIME detection |

## Architecture

```
DocumentSource ──► Reader (async, file I/O)  ──┐
                                                ├──► Document ──► pipeline
               ──► Extractor (sync, bytes-in) ──┘

ExtractorRegistry
  ├── extractor_for_extension("pdf")    → PdfExtractor
  ├── extractor_for_mime("text/html")   → HtmlExtractor
  ├── extractor_for_path("doc.docx")    → DocxExtractor
  ├── extractor_for_bytes(buf, hint)    → MIME detection first, then extension
  └── all_capabilities()               → Vec<ReaderCapabilities>
```

## Design decisions

- **`Extractor` is synchronous**: bytes are already in memory, so there is no I/O. CPU-bound callers (PDF, DOCX) use `spawn_blocking` at the Reader level, not inside the Extractor.
- **`Document.metadata` changed from `Value` to `DocumentMetadata`**: `Chunk.metadata` stays `Value` because chunk-level metadata (page, offset, token count) is genuinely open-ended.
- **Extractor helpers reused from readers**: `extract_text_and_title`, `extract_body_text`, `extract_text_runs`, `extract_core_props` made `pub` in their reader modules. No duplication.
- **`infer` MIME detection as first-class dispatch**: prevents disguised files from reaching the wrong extractor.
- **`DocumentValidator` is standalone**: not wired into `IngestPipeline` yet. Callers invoke it explicitly; a future pipeline phase will add it as an optional gate between Reader and Chunker.

## Validation

```
cargo test -p valori-ingest  → 89 passed, 0 failed (was 49 before E3.1–E3.6)
cargo build -p valori-node   → 0 errors
```

New tests by module:
- `metadata.rs` — 0 (tested indirectly through readers/extractors)
- `extractor/text` — 3
- `extractor/markdown` — 2
- `extractor/html` — 1
- `extractor/pdf` — 2
- `extractor/docx` — 2
- `extractor_registry` — 15
- `validator` — 8
- `source` — 8

## Follow-ups

- Wire `DocumentValidator` into `IngestPipeline` as an optional gate stage.
- Protected-PDF detection: currently uses `mime_type == "application/pdf+protected"` placeholder; real detection needs `lopdf` to surface the `Encrypt` dict.
- Language detection: `DocumentMetadata.language` field exists but is not populated; a `whatlang`/`lingua` crate can fill it in a future phase.
- `DocxExtractor`: count pages by counting `<w:sectPr>` elements in `document.xml`.
