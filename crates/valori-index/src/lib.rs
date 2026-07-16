// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! Vector index structures for the Valori platform.
//!
//! # Design invariants
//!
//! - All indexes implement [`VectorIndex`] — one uniform interface for build / search / insert / delete / snapshot / restore.
//! - No dependency on `valori-node` or any HTTP layer. Pure computation only.
//! - NEON SIMD distance kernels are gated behind `#[cfg(target_arch = "aarch64")]`; the
//!   scalar fallback is always present.
//! - Determinism: K-Means seeding and tie-breaking use FNV hashes + id ascending —
//!   bit-identical results on x86/ARM/WASM.

pub mod bq;
pub mod brute_force;
pub mod deterministic;
pub mod hnsw;
pub mod ivf;
pub mod quant;
pub mod traits;

pub use bq::BqIndex;
pub use brute_force::BruteForceIndex;
pub use deterministic::kmeans::{deterministic_kmeans, f32_to_q16, l2_sq_q16};
pub use hnsw::{HnswConfig, HnswIndex};
pub use ivf::{IvfConfig, IvfIndex};
pub use quant::pq::{PqConfig, ProductQuantizer};
pub use quant::{NoQuantizer, Quantizer, ScalarQuantizer};
pub use traits::VectorIndex;
