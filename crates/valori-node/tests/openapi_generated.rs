// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! The generated OpenAPI document versus the committed contract.
//!
//! Run with `cargo test -p valori-node --features utoipa`. Without the
//! feature the whole file compiles to nothing, so the default test run is
//! unaffected.
//!
//! # What changed in Phase API-3.1
//!
//! This file used to assert something much weaker: that every schema utoipa
//! generated *also appeared somewhere* in the committed contract, with an
//! allowlist for the ones that did not. That was the right test while the
//! contract was hand-maintained and generation covered a fraction of it — but
//! it could not have caught the Phase API-3 failure, because a hand-written
//! document that is a strict superset of the generated one passes it.
//!
//! Now that `api/openapi/valori-v1.yaml` is produced end-to-end by
//! `valori-openapi`, the honest assertion is **byte equality**. If the
//! committed file is not exactly what the generator emits, something outside
//! the Rust build edited it, and that is the one failure mode this whole
//! workstream exists to prevent.

#![cfg(feature = "utoipa")]

const COMMITTED: &str = include_str!("../../../api/openapi/valori-v1.yaml");

#[test]
fn committed_contract_is_byte_identical_to_the_generator_output() {
    let generated = valori_node::openapi::to_yaml().expect("utoipa failed to render the document");

    if generated == COMMITTED {
        return;
    }

    // Point at the first divergence rather than dumping two 6000-line files.
    let (mut line, mut g, mut c) = (0usize, generated.lines(), COMMITTED.lines());
    loop {
        line += 1;
        match (g.next(), c.next()) {
            (None, None) => break,
            (a, b) if a == b => continue,
            (a, b) => panic!(
                "api/openapi/valori-v1.yaml is not the generator's output.\n\
                 First divergence at line {line}:\n\
                 \x20 generated: {a:?}\n\
                 \x20 committed: {b:?}\n\n\
                 Regenerate it — never hand-edit:\n\
                 \x20 cargo run -p valori-node --features utoipa --bin valori-openapi -- \\\n\
                 \x20     --output api/openapi/valori-v1.yaml"
            ),
        }
    }
}

#[test]
fn generated_document_declares_the_target_openapi_version() {
    let doc: serde_norway::Value =
        serde_norway::from_str(&valori_node::openapi::to_yaml().expect("render"))
            .expect("generated document is valid YAML");

    // The target is 3.1.0 as of Phase API-3.1; utoipa 5.x cannot emit 3.0.x at
    // all. See docs/api/openapi-version-decision.md for why that became the
    // target rather than something to be converted away from.
    assert_eq!(
        doc["openapi"].as_str(),
        Some("3.1.0"),
        "generated document declares an unexpected OpenAPI version"
    );
}

#[test]
fn the_error_taxonomy_is_a_first_class_component() {
    let doc: serde_norway::Value =
        serde_norway::from_str(&valori_node::openapi::to_yaml().expect("render")).expect("yaml");
    let schemas = doc["components"]["schemas"]
        .as_mapping()
        .expect("components.schemas");

    // §20: both halves of the error contract are generated components, and
    // ErrorCode is a closed string enum, not a free-form string.
    assert!(
        schemas.contains_key("ApiError"),
        "ApiError is not generated"
    );
    let code = &doc["components"]["schemas"]["ErrorCode"];
    let variants = code["enum"]
        .as_sequence()
        .expect("ErrorCode is not an enum in the generated document");
    assert!(
        variants.len() >= 10,
        "ErrorCode has only {} variants — the taxonomy is truncated",
        variants.len()
    );
    assert_eq!(code["type"].as_str(), Some("string"));

    let props = doc["components"]["schemas"]["ApiError"]["properties"]
        .as_mapping()
        .expect("ApiError.properties");
    for field in ["error", "code"] {
        assert!(
            props.contains_key(field),
            "ApiError is missing the `{field}` field"
        );
    }
}

#[test]
fn generation_is_deterministic() {
    // The vendor-extension pass writes into a `HashMap`, whose iteration order
    // is randomised per process. `to_yaml` normalises through serde_json so
    // that cannot leak into the artifact; this pins that guarantee.
    let a = valori_node::openapi::to_yaml().expect("render");
    let b = valori_node::openapi::to_yaml().expect("render");
    assert_eq!(a, b, "two renders in one process disagree");
}
