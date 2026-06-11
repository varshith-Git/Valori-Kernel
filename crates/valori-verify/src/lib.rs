// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! Library surface of `valori-verify` — exposes the wire-format mirror so
//! integration tests (and external auditors' tooling) can use the exact
//! decoding logic the verifier binaries use.
//!
//! The binaries (`valori-verify`, `make-demo-log`, `valori-anchor`) include
//! these modules directly; this lib target adds no behavior of its own.

pub mod wire;
