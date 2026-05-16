# Valori Kernel: Module Analysis - Python SDK & Ecosystem Integrations

This is the final piece of the Valori architecture. We have successfully traced a vector from its deterministic mathematical core (`src/math/`), through the structural storage (`src/storage/`), up into the Server Orchestrator (`node/src/`). Now, we look at the very top: the **Python SDK Client (`python/valoricore/`)**.

This is the code that user applications actually import and run.

---

## 1. Remote Client Protocol

**Location**: `python/valoricore/remote.py`

This module is responsible for bridging the gap between Python scripts and the Rust Axum HTTP server.

### `SyncRemoteClient` and `AsyncRemoteClient`
Valori provides both blocking (`requests`) and non-blocking (`httpx`) clients.
- **`insert_with_proof`**: A specialized ingestion method that handles Valori's unique cryptographic capabilities. Before sending the embedding over the wire, it uses the locally-compiled C-FFI (`valoricore_ffi.so`) to locally compute the `generate_proof` hex string. It then uploads the vector, gets the `RecordId`, and returns the local proof bytes so the user can verify the remote server didn't tamper with the embedding.
- **Raw Methods**: Directly exposes `/records`, `/search`, `/graph/node`, and `/snapshot`.
- **Typing**: Uses custom typing definitions (`Vector`, `RecordId`) to enforce strict Float lists and integers before network transmission.

---

## 2. High-Level Operations (The Adapter)

**Location**: `python/valoricore/base.py`

While the Remote client handles raw bytes and JSON dictionaries, the `ValoricoreAdapter` acts as the user-friendly entry point.

### `ValoricoreAdapter`
- **Automatic Embedding (`upsert_document`)**: If a user provides raw text instead of a vector, the adapter automatically invokes a configured `embed_fn` (e.g., an OpenAI API call or a local SentenceTransformer model) to generate the embedding before uploading it.
- **Text-to-Metadata Injection**: Because Valori only stores vectors natively, the SDK must handle the actual text content. It takes the original string `text`, injects it into the `metadata` dictionary (`full_metadata["text"] = text`), and uploads the whole payload. When searching, the SDK extracts `"text"` from the returned metadata.
- **Resilience**: Implements automatic exponential backoff (`_retry`) for network requests, ensuring production stability.
- **Float Validation**: Validates that floats are within safe limits (`validate_float_range`) before sending them to the Rust backend, preventing overflow panics in the Q16.16 logic.

---

## 3. Ecosystem Adapters (LangChain Integration)

**Location**: `python/valoricore/langchain_vectorstore.py`

Valori is designed to be a drop-in replacement for non-deterministic databases like FAISS, Chroma, or Pinecone. To achieve this, it implements the official LangChain `VectorStore` interface.

### `ValoricoreVectorStore(VectorStore)`
- **`add_texts`**: Takes LangChain's standard `List[str]` and `List[dict]` inputs. It calls the LangChain `Embeddings` model to vectorize the text, merges the text into the metadata, and pushes it through the `ValoricoreAdapter`.
- **`similarity_search`**: Reverses the process. It takes a query string, embeds it, queries the Rust node for the top `k` hits, extracts the `"text"` field from the returned metadata, and rebuilds LangChain `Document` objects.
- **Metadata Scores**: It enriches the LangChain `Document.metadata` with the deterministic `score` distance and the physical memory array slot `memory_id` returned by the kernel.
- **Significance**: Because it perfectly wraps the LangChain interface, a developer can swap out `FAISS` for `ValoricoreVectorStore` by changing exactly one line of code, immediately upgrading their RAG (Retrieval-Augmented Generation) application to be cryptographically auditable and crash-resilient.

---

### Summary of Module Architecture
1. **Separation of Concerns**: The Python layer is strictly responsible for HTTP formatting, JSON parsing, API retry logic, and interacting with third-party LLM frameworks. It **never** attempts to do vector math or array sorting, delegating 100% of the determinism to the Rust backend.
2. **FFI Acceleration**: When cryptographic operations (like Merkle leaf hashing) need to happen *on the client side* (to maintain zero-trust architectures), Python delegates down to a compiled Rust FFI binary (`valoricore_ffi.so`) rather than doing it natively, ensuring the client exactly mirrors the server's BLAKE3 logic.
