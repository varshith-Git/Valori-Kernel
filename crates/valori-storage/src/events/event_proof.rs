// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! Event log file hashing — used by the `/v1/proof/event-log` endpoint.

/// Compute the BLAKE3 hash of the raw event log file at `path`.
///
/// This is a file-level hash (bytes on disk), not the BLAKE3 chain head that
/// `valori-verify` computes entry-by-entry. Use `valori_verify::verify_log_file`
/// for end-to-end audit; use this for quick integrity checks at the HTTP layer.
pub fn compute_event_log_hash(path: impl AsRef<std::path::Path>) -> std::io::Result<[u8; 32]> {
    use std::fs::File;
    use std::io::Read;

    let mut file = File::open(path)?;
    let mut hasher = blake3::Hasher::new();
    let mut buffer = [0u8; 8192];

    loop {
        let bytes_read = file.read(&mut buffer)?;
        if bytes_read == 0 {
            break;
        }
        hasher.update(&buffer[..bytes_read]);
    }

    Ok(*hasher.finalize().as_bytes())
}
