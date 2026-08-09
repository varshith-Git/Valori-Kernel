// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! Validating `Deserialize` for newtypes with a canonical parsing constructor.
//!
//! # Why this exists
//!
//! A `#[serde(transparent)]` newtype over `String` derives a `Deserialize` that
//! deserializes the inner primitive and wraps it — **the constructor never
//! runs**. Every invariant the type advertises is therefore unenforced on the
//! one path untrusted input actually takes: an HTTP body, a `project.json`, a
//! redb value.
//!
//! This was a real defect, not a theoretical one. Before this module,
//! `serde_json::from_str::<ProjectName>("\"../../etc/passwd\"")` succeeded,
//! while `ProjectName::parse("../../etc/passwd")` correctly failed — and
//! `ProjectName` is used as a directory name by all three project
//! implementations. See `docs/reviews/m2-project-review.md` finding F1.
//!
//! # The rule
//!
//! Validation lives in exactly one place: the type's `parse()` constructor.
//! [`validating_deserialize!`] routes `Deserialize` through it, so the two can
//! never drift. Never re-express a rule inside a `Deserialize` impl.
//!
//! # Wire compatibility
//!
//! `Serialize` is untouched — it stays `#[serde(transparent)]`, so the emitted
//! JSON is byte-identical to before. Only the *acceptance* of input changed:
//! values that were always invalid are now rejected instead of silently
//! admitted.
//!
//! # Types that do not need this
//!
//! A newtype whose inner primitive already validates is safe without it, and
//! wrapping it would add indirection for nothing:
//!
//! - `ProjectId` / `SessionId` / `InstallationId` — `Uuid`'s own `Deserialize`
//!   rejects malformed UUIDs.
//! - `ProjectTopology` — `NonZeroU8`'s own `Deserialize` rejects `0`.
//!
//! `tests/invariants.rs` asserts that for each of them, so the assumption is
//! checked rather than believed.

/// Implement `Deserialize` for a newtype by routing through its `parse()`.
///
/// The type must expose `pub fn parse(impl Into<String>) -> crate::Result<Self>`.
/// Deserialization errors carry the `DomainError`'s message, so an API returns
/// the same diagnostic a direct `parse()` would have produced.
macro_rules! validating_deserialize {
    ($name:ident) => {
        impl<'de> serde::Deserialize<'de> for $name {
            fn deserialize<D>(deserializer: D) -> core::result::Result<Self, D::Error>
            where
                D: serde::Deserializer<'de>,
            {
                let raw = String::deserialize(deserializer)?;
                // The single source of truth for this type's invariants.
                Self::parse(raw).map_err(serde::de::Error::custom)
            }
        }
    };
}

pub(crate) use validating_deserialize;
