// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! Monotonic version counter used for schema and data versioning.

use serde::{Deserialize, Serialize};

/// Monotonically increasing version counter.
///
/// Used by snapshots, event logs, and wire formats to detect schema
/// incompatibilities. `Version(0)` means "unversioned / legacy".
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Default,
         Serialize, Deserialize)]
#[repr(transparent)]
pub struct Version(pub u64);

impl Version {
    pub const ZERO: Self = Version(0);

    pub fn next(self) -> Self {
        Version(self.0.checked_add(1).expect("Version overflow"))
    }
}

impl core::fmt::Display for Version {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "v{}", self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_monotonic() {
        let v = Version(3);
        assert_eq!(v.next(), Version(4));
    }

    #[test]
    fn version_display() {
        assert_eq!(Version(5).to_string(), "v5");
    }
}
