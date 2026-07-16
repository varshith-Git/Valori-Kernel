//! Fuzz target: interpret arbitrary bytes as UTF-8 JSON, deserialize as
//! KernelEvent, and apply it to a fresh KernelState.
//!
//! The kernel must never panic on any input — every bad event must
//! produce Ok or Err, never trigger an unwrap/index-out-of-bounds abort.
#![no_main]

use libfuzzer_sys::fuzz_target;
use valori_kernel::state::kernel::KernelState;
use valori_kernel::event::KernelEvent;

fuzz_target!(|data: &[u8]| {
    let Ok(s) = std::str::from_utf8(data) else { return };
    let Ok(evt) = serde_json::from_str::<KernelEvent>(s) else { return };
    let mut state = KernelState::new();
    let _ = state.apply_event(&evt);
});
