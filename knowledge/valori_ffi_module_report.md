# Valori Kernel: Module Analysis - Foreign Function Interface (FFI)

You asked about the FFI layer. This is an excellent catch! It turns out there is a critical "invisible" boundary between the Rust Kernel and the Python SDK: **The Foreign Function Interface (`ffi/src/lib.rs`)**.

Because Valori aims to be a mathematically perfect, determinism-first engine, it cannot trust Python to correctly round floats or serialize bytes. Instead, it compiles a C-extension using `PyO3` (`valoricore_ffi.so`) that Python imports.

---

## 1. Bridging the Boundary (`ValoricoreEngine`)

**Location**: `ffi/src/lib.rs`

The FFI exposes the `ValoricoreEngine` class directly to Python. This acts as a wrapper around the `Engine` orchestrator we saw earlier.

### Memory Isolation
```rust
#[pyclass]
struct ValoricoreEngine {
    inner: Arc<Mutex<Engine>>,
}
```
By wrapping the engine in a Rust `Arc<Mutex>`, the FFI guarantees that no matter what the Python Global Interpreter Lock (GIL) does, or how many asynchronous Python threads hit the local DB simultaneously, the internal memory pool remains perfectly thread-safe and sequentially consistent.

### Networkless Operation
If a user does not want to run the standalone Axum server, they can initialize `ValoricoreEngine("path/to/db")` directly in Python. The FFI links the Rust binary straight into the Python process, giving the user in-memory database speeds while writing strictly deterministic `.log` and `.wal` files to the local disk.

---

## 2. Mathematical Boundary Enforcement

Python uses IEEE 754 floating-point representations. Valori uses Q16.16 fixed-point representation. The translation between these two formats must happen in Rust.

### `ingest_embedding`
```rust
#[pyfunction]
fn ingest_embedding(floats: Vec<f32>) -> PyResult<Vec<i32>> {
    for (i, &f) in floats.iter().enumerate() {
        if f < -32767.0 || f > 32767.0 {
            return Err(PyValueError::new_err("Outside valid range"));
        }
    }
    // ... converts to i32 fixed point
}
```
Before any vector math can happen, the FFI rigorously scans the incoming float array.
- **Safety**: Q16.16 integer mathematics maxes out slightly below `32768.0`. If Python passes a float larger than this, it would overflow the `i32` bounds, destroying the deterministic hashing. The FFI acts as a firewall, immediately rejecting illegal numbers.

---

## 3. Zero-Trust Proof Generation

The core philosophy of Valori is that the client should not have to blindly trust the server. If the server says "I saved your embedding", it must provide a cryptographic receipt.

### `generate_proof` and `verify_embedding`
```rust
#[pyfunction]
fn generate_proof(fixed_values: Vec<i32>) -> PyResult<String> {
    Ok(hex::encode(generate_proof_bytes(&fixed_values)))
}

#[pyfunction]
fn verify_embedding(floats: Vec<f32>, claimed_hash: String) -> PyResult<bool> {
    let fixed = ingest_embedding(floats)?;
    let computed_hash = generate_proof(fixed)?;
    Ok(computed_hash == claimed_hash)
}
```
These functions are exposed to the Python client so they can execute *locally* on the user's machine:
1. The user's Python SDK takes their raw vector and calls the local `valoricore_ffi.generate_proof()`.
2. The FFI runs the exact same `BLAKE3` Merkle Tree algorithm that the server uses.
3. The user uploads the vector to the remote server.
4. The server responds with its own computed hash.
5. The Python SDK calls `valoricore_ffi.verify_embedding()` to assert that the locally computed hash perfectly matches the remote server's hash. 

If they match, the user has mathematical certainty that the server did not mutate, truncate, or inject noise into the embedding during network transmission or storage.

---

### Summary of FFI Capabilities
1. **Performance**: Translating Python floats to Rust `i32` fixed-point arrays happens natively in C/Rust, bypassing Python loop bottlenecks entirely.
2. **Zero-Trust**: By distributing the `valoricore_ffi.so` binary with the Python client, users possess the identical hashing algorithms as the server, enabling trustless, verifiable vector storage without needing to rewrite complex cryptographic Merkle trees in pure Python.
