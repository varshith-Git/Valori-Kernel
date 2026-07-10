// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! Configuration constants.

/// Number of fractional bits for Fixed-Point representation (Q16.16).
pub const FRAC_BITS: u32 = 16;

/// Scaling factor for Fixed-Point representation (1 << FRAC_BITS).
pub const SCALE: i32 = 1 << FRAC_BITS;

/// Maximum size in bytes for a single record's metadata blob.
pub const MAX_METADATA_SIZE: usize = 64 * 1024; // 64 KiB

/// Maximum vector dimension accepted at insert time and during snapshot decode.
/// Prevents OOM from a crafted snapshot with a huge dim field.
/// 65 536 dimensions × 4 bytes = 256 KiB per vector — already very generous.
pub const MAX_DIM: usize = 65_536;

/// Maximum number of record slots in a snapshot.
/// 10 M records × (4 B id + 256 KiB vector + 64 KiB meta) ≈ tight; enforce a
/// hard ceiling that fits within reasonable server RAM before the file is parsed.
pub const MAX_RECORDS: usize = 10_000_000;

/// Maximum number of graph nodes in a snapshot.
/// Graphs can be denser than records, but 50 M nodes is already extreme.
pub const MAX_NODES: usize = 50_000_000;

/// Maximum number of graph edges in a snapshot.
pub const MAX_EDGES: usize = 200_000_000;

/// Maximum number of key-value pairs in the V7 `KernelState.meta` section.
pub const MAX_META_ENTRIES: usize = 1_000_000;
