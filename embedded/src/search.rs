use valori_kernel::state::kernel::KernelState;
use valori_kernel::index::SearchResult;
use valori_kernel::types::vector::FxpVector;
use valori_kernel::types::scalar::FxpScalar;
use valori_kernel::types::id::RecordId;
use valori_kernel::verify::kernel_state_hash;
use crate::transport;

/// Maximum k the firmware will serve in one query.
pub const MAX_K: usize = 8;

// ── Request parsing ───────────────────────────────────────────────────────────

struct SearchRequest {
    namespace_id: u16,
    k:            usize,
    query:        FxpVector,
}

/// Payload layout: [NS:2 LE][K:1][SCALAR_0..SCALAR_{DIM-1}: each i32 LE]
/// Total minimum bytes: 3 + DIM*4
fn parse_request(payload: &[u8]) -> Option<SearchRequest> {
    let dim = crate::DIM;
    if payload.len() < 3 + dim * 4 { return None; }

    let ns = u16::from_le_bytes([payload[0], payload[1]]);
    let k  = payload[2] as usize;
    if k == 0 || k > MAX_K { return None; }

    let mut query = FxpVector::new_zeros(dim);
    for i in 0..dim {
        let b = 3 + i * 4;
        let raw = i32::from_le_bytes([payload[b], payload[b+1], payload[b+2], payload[b+3]]);
        query.data[i] = FxpScalar(raw);
    }

    Some(SearchRequest { namespace_id: ns, k, query })
}

// ── Handler ───────────────────────────────────────────────────────────────────

/// Parse a search request from `payload`, run `search_l2_ns`, and export a
/// framed `TYPE_SEARCH_RESULT` packet.
///
/// Result payload layout:
///   [K_FOUND:1][VERSION:8 LE][{ID:4 LE, SCORE:4 LE} × K_FOUND][STATE_HASH:32]
///
/// The STATE_HASH proves exactly which kernel state was searched — the host
/// can verify this against the node's `/v1/proof` endpoint.
pub fn handle(state: &KernelState, payload: &[u8]) {
    let req = match parse_request(payload) {
        Some(r) => r,
        None => { transport::export_error(b"BAD_SEARCH"); return; }
    };

    // Stack-allocate result slots (MAX_K × 8 bytes = 64 bytes max).
    let mut results = [SearchResult { score: FxpScalar(i32::MAX), id: RecordId(u32::MAX) }; MAX_K];
    let found = state.search_l2_ns(&req.query, &mut results[0..req.k], req.namespace_id);

    let state_hash = kernel_state_hash(state);
    let version    = state.version();

    // Encode result packet — no heap needed.
    // Max size: 1 + 8 + MAX_K*8 + 32 = 105 bytes.
    let mut buf = [0u8; 1 + 8 + MAX_K * 8 + 32];
    let mut off = 0;

    buf[off] = found as u8;                        off += 1;
    buf[off..off+8].copy_from_slice(&version.to_le_bytes()); off += 8;

    for i in 0..found {
        let r = results[i];
        buf[off..off+4].copy_from_slice(&r.id.0.to_le_bytes());
        buf[off+4..off+8].copy_from_slice(&(r.score.0 as u32).to_le_bytes());
        off += 8;
    }

    buf[off..off+32].copy_from_slice(&state_hash); off += 32;

    transport::export_search_result(&buf[0..off]);
}
