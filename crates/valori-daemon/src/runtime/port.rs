// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! Port allocation — one job: hand out a free TCP port in a window.

use std::collections::HashSet;
use std::net::TcpListener;

use crate::error::{DaemonError, DaemonResult};

/// Allocates ports in `[lo, hi]`, skipping any already handed out and any the
/// OS reports as in use.
pub struct PortAllocator {
    lo: u16,
    hi: u16,
}

impl PortAllocator {
    pub fn new(lo: u16, hi: u16) -> Self {
        Self { lo, hi }
    }

    pub fn range(&self) -> (u16, u16) {
        (self.lo, self.hi)
    }

    /// First free port not in `taken`.
    pub fn allocate(&self, taken: &HashSet<u16>) -> DaemonResult<u16> {
        for p in self.lo..=self.hi {
            if taken.contains(&p) {
                continue;
            }
            if TcpListener::bind(("127.0.0.1", p)).is_ok() {
                return Ok(p);
            }
        }
        Err(DaemonError::NoFreePort)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allocates_and_skips_taken() {
        let alloc = PortAllocator::new(8100, 8110);
        let first = alloc.allocate(&HashSet::new()).unwrap();
        assert!((8100..=8110).contains(&first));
        let mut taken = HashSet::new();
        taken.insert(first);
        let second = alloc.allocate(&taken).unwrap();
        assert_ne!(first, second);
    }
}
