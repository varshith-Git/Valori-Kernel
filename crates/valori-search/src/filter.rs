// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! Metadata predicate matching for post-retrieval filtering.
//!
//! A `MetadataFilter` is a JSON object whose keys are field names and whose
//! values are either:
//!
//! - **Exact match** — any JSON scalar (string, number, bool, null).
//!   The record's field must equal the filter value exactly.
//! - **Range operator** — a JSON object with one or more of `eq`, `gt`, `gte`,
//!   `lt`, `lte`. All operators that appear must pass simultaneously. Only
//!   numeric fields support range operators; applying them to a non-numeric
//!   field returns `false`.
//!
//! All keys in the filter must be present **and** matching in the record's
//! metadata for the record to pass. Missing keys always fail.
//!
//! # Examples
//!
//! ```json
//! // Exact match
//! {"author": "Alice"}
//!
//! // Numeric range
//! {"year": {"gte": 2020, "lte": 2024}}
//!
//! // Combined
//! {"author": "Alice", "year": {"gte": 2020}}
//! ```

use serde_json::{Map, Value};

/// A JSON predicate object. Every key-value pair must match for a record to pass.
pub type MetadataFilter = Map<String, Value>;

/// Returns `true` when every key in `filter` is present in `meta` and its
/// value satisfies the predicate (exact or range).
///
/// `meta` must be a JSON object; any other shape returns `false`.
pub fn matches_metadata_filter(meta: &Value, filter: &MetadataFilter) -> bool {
    let obj = match meta.as_object() {
        Some(o) => o,
        None => return false,
    };
    filter.iter().all(|(key, expected)| {
        obj.get(key)
            .map(|actual| value_matches(actual, expected))
            .unwrap_or(false)
    })
}

/// Evaluate one field value against one filter predicate.
fn value_matches(actual: &Value, expected: &Value) -> bool {
    if let Some(ops) = expected.as_object() {
        if is_range_predicate(ops) {
            return apply_range(actual, ops);
        }
    }
    actual == expected
}

/// True when the object contains at least one recognised operator key.
#[inline]
fn is_range_predicate(ops: &Map<String, Value>) -> bool {
    ops.contains_key("eq")
        || ops.contains_key("gt")
        || ops.contains_key("gte")
        || ops.contains_key("lt")
        || ops.contains_key("lte")
}

/// Apply numeric range operators. Returns `false` if `actual` is not a number.
#[allow(clippy::neg_cmp_op_on_partial_ord)]
fn apply_range(actual: &Value, ops: &Map<String, Value>) -> bool {
    let num = match actual.as_f64() {
        Some(n) => n,
        None => return false,
    };

    if let Some(v) = ops.get("eq") {
        if actual != v {
            return false;
        }
    }
    if let Some(v) = ops.get("gt").and_then(Value::as_f64) {
        if !(num > v) {
            return false;
        }
    }
    if let Some(v) = ops.get("gte").and_then(Value::as_f64) {
        if !(num >= v) {
            return false;
        }
    }
    if let Some(v) = ops.get("lt").and_then(Value::as_f64) {
        if !(num < v) {
            return false;
        }
    }
    if let Some(v) = ops.get("lte").and_then(Value::as_f64) {
        if !(num <= v) {
            return false;
        }
    }
    true
}

// ── tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn meta(v: serde_json::Value) -> Value {
        v
    }

    #[test]
    fn exact_string_match() {
        let m = meta(json!({"author": "Alice", "year": 2023}));
        let f: MetadataFilter = serde_json::from_value(json!({"author": "Alice"})).unwrap();
        assert!(matches_metadata_filter(&m, &f));
    }

    #[test]
    fn exact_string_mismatch() {
        let m = meta(json!({"author": "Bob"}));
        let f: MetadataFilter = serde_json::from_value(json!({"author": "Alice"})).unwrap();
        assert!(!matches_metadata_filter(&m, &f));
    }

    #[test]
    fn missing_key_fails() {
        let m = meta(json!({"year": 2023}));
        let f: MetadataFilter = serde_json::from_value(json!({"author": "Alice"})).unwrap();
        assert!(!matches_metadata_filter(&m, &f));
    }

    #[test]
    fn non_object_meta_fails() {
        let m = meta(json!("not an object"));
        let f: MetadataFilter = serde_json::from_value(json!({"author": "Alice"})).unwrap();
        assert!(!matches_metadata_filter(&m, &f));
    }

    #[test]
    fn numeric_gte_passes() {
        let m = meta(json!({"year": 2022}));
        let f: MetadataFilter = serde_json::from_value(json!({"year": {"gte": 2020}})).unwrap();
        assert!(matches_metadata_filter(&m, &f));
    }

    #[test]
    fn numeric_gte_fails() {
        let m = meta(json!({"year": 2019}));
        let f: MetadataFilter = serde_json::from_value(json!({"year": {"gte": 2020}})).unwrap();
        assert!(!matches_metadata_filter(&m, &f));
    }

    #[test]
    fn numeric_range_both_bounds() {
        let m = meta(json!({"year": 2022}));
        let f: MetadataFilter =
            serde_json::from_value(json!({"year": {"gte": 2020, "lte": 2024}})).unwrap();
        assert!(matches_metadata_filter(&m, &f));

        let m_out = meta(json!({"year": 2025}));
        assert!(!matches_metadata_filter(&m_out, &f));
    }

    #[test]
    fn all_keys_must_match() {
        let m = meta(json!({"author": "Alice", "year": 2022}));
        let f: MetadataFilter =
            serde_json::from_value(json!({"author": "Alice", "year": {"gte": 2023}})).unwrap();
        assert!(!matches_metadata_filter(&m, &f), "year fails ≥2023 check");
    }

    #[test]
    fn range_on_non_numeric_fails() {
        let m = meta(json!({"tag": "rust"}));
        let f: MetadataFilter = serde_json::from_value(json!({"tag": {"gte": 1}})).unwrap();
        assert!(!matches_metadata_filter(&m, &f));
    }

    #[test]
    fn empty_filter_matches_everything() {
        let m = meta(json!({"author": "Alice"}));
        let f: MetadataFilter = serde_json::from_value(json!({})).unwrap();
        assert!(matches_metadata_filter(&m, &f));
    }

    #[test]
    fn gt_strict_boundary() {
        let m = meta(json!({"score": 5.0}));
        let f_gt: MetadataFilter = serde_json::from_value(json!({"score": {"gt": 5.0}})).unwrap();
        let f_gte: MetadataFilter = serde_json::from_value(json!({"score": {"gte": 5.0}})).unwrap();
        assert!(!matches_metadata_filter(&m, &f_gt), "5 > 5 is false");
        assert!(matches_metadata_filter(&m, &f_gte), "5 >= 5 is true");
    }
}
