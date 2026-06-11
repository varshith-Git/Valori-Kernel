// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! Library surface of `valori-verify`.
//!
//! The wire format lives in the `valori-wire` crate (shared with the node
//! and the forensic CLI — one definition, no drift). Re-exported here so
//! auditors' tooling and the integration tests reach it through this crate.

pub use valori_wire as wire;
