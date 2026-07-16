// ARCHITECTURAL INVARIANT: valori-kernel must remain no_std.
// Never add `use std::` to any file in this crate. Use `core::` or `alloc::` instead.
// Anything requiring std must be gated behind `#[cfg(feature = "std")]`.
// Verify after every change: `cargo build -p valori-kernel --target wasm32-unknown-unknown`
#![cfg_attr(not(feature = "std"), no_std)]
extern crate alloc;
pub mod config;
pub mod crypto;
pub mod error;
pub mod event;
pub mod fxp;
pub mod graph;
pub mod types;
pub mod snapshot;
#[cfg(feature = "std")]
pub mod adapters;
pub mod index;
pub mod math;
pub mod proof;
pub mod state;
pub mod storage;
pub mod verify;
