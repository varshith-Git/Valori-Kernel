//! Fuzz target: feed arbitrary bytes as a snapshot and attempt to decode.
//!
//! The decoder must never panic on malformed input — invalid magic,
//! truncated data, corrupt checksums, or oversized fields must all
//! produce Err, never panic.
#![no_main]

use libfuzzer_sys::fuzz_target;
use valori_kernel::snapshot::decode::decode_state;

fuzz_target!(|data: &[u8]| {
    // decode_state must not panic on any byte sequence.
    let _ = decode_state(data);
});
