// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! Legacy module — state hashing is handled exclusively by
//! `snapshot::blake3::hash_state_blake3`. The former FNV-1a `hash_state`
//! function has been removed (64-bit non-crypto, no domain separation,
//! ambiguous None-vs-empty-metadata representation).
