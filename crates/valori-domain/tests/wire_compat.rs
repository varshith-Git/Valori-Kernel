// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! Wire-shape and round-trip contract for the domain identity types.
//!
//! Every type in `valori-domain` appears in `project.json` on disk, in HTTP
//! JSON, and in Cloud persistence. A change to any assertion in this file is a
//! **compatibility break** governed by `COMPATIBILITY.md`, not a test that
//! needs updating.
//!
//! The most important property asserted here is *transparency*: each ID
//! serializes as the bare primitive it wraps. That is what allows these types
//! to replace today's raw `String` fields without rewriting a single existing
//! manifest.

use std::str::FromStr;

use valori_domain::{DomainError, InstallationId, ModelId, ProjectId, SessionId, SnapshotId};

// ── Transparency: the wire form is the primitive, not an object ───────────────

#[test]
fn uuid_ids_serialize_as_bare_strings() {
    let id = ProjectId::new();
    let json = serde_json::to_string(&id).expect("ProjectId serializes");

    assert_eq!(
        json,
        format!("\"{id}\""),
        "ProjectId must serialize transparently as a bare UUID string. A \
         wrapped shape like {{\"0\":\"…\"}} would break every existing \
         project.json, whose `id` field is a plain string."
    );
    assert!(!json.contains('{'), "no object wrapper: {json}");
}

#[test]
fn string_ids_serialize_as_bare_strings() {
    let model = ModelId::parse("openai/text-embedding-3-small").unwrap();
    assert_eq!(
        serde_json::to_string(&model).unwrap(),
        "\"openai/text-embedding-3-small\"",
        "ModelId must match ModelManifest.id's existing on-disk String form"
    );

    let snap = SnapshotId::parse("snapshots/00000001750000000_abc12345.snap").unwrap();
    assert_eq!(
        serde_json::to_string(&snap).unwrap(),
        "\"snapshots/00000001750000000_abc12345.snap\"",
        "SnapshotId must match the object key the storage layer emits"
    );
}

#[test]
fn ids_deserialize_from_the_string_forms_already_on_disk() {
    // Exactly what `daemon::new_id()` writes into project.json today.
    let raw = "\"7c9e6679-7425-40de-944b-e07fc1f90ae7\"";
    let id: ProjectId = serde_json::from_str(raw).expect("parses today's manifest form");
    assert_eq!(format!("\"{id}\""), raw);

    let model: ModelId = serde_json::from_str("\"ollama/nomic-embed-text\"").unwrap();
    assert_eq!(model.as_str(), "ollama/nomic-embed-text");
}

// ── Round-trips ───────────────────────────────────────────────────────────────

#[test]
fn every_id_round_trips_through_json() {
    let project = ProjectId::new();
    let session = SessionId::new();
    let install = InstallationId::new();
    let model = ModelId::parse("openai/text-embedding-3-small").unwrap();
    let snapshot = SnapshotId::parse("snapshots/00000001750000000_abc12345.snap").unwrap();

    macro_rules! round_trip {
        ($value:expr, $ty:ty) => {{
            let json = serde_json::to_string(&$value).unwrap();
            let back: $ty = serde_json::from_str(&json).unwrap();
            assert_eq!($value, back, "round-trip changed the value: {json}");
        }};
    }

    round_trip!(project, ProjectId);
    round_trip!(session, SessionId);
    round_trip!(install, InstallationId);
    round_trip!(model.clone(), ModelId);
    round_trip!(snapshot.clone(), SnapshotId);
}

#[test]
fn every_id_round_trips_through_display_and_from_str() {
    let project = ProjectId::new();
    assert_eq!(project, ProjectId::from_str(&project.to_string()).unwrap());

    let session = SessionId::new();
    assert_eq!(session, SessionId::from_str(&session.to_string()).unwrap());

    let install = InstallationId::new();
    assert_eq!(
        install,
        InstallationId::from_str(&install.to_string()).unwrap()
    );

    let model = ModelId::parse("ollama/nomic-embed-text").unwrap();
    assert_eq!(model, ModelId::from_str(&model.to_string()).unwrap());

    let snapshot = SnapshotId::parse("snapshots/x_y.snap").unwrap();
    assert_eq!(
        snapshot,
        SnapshotId::from_str(&snapshot.to_string()).unwrap()
    );
}

// ── Identity semantics ────────────────────────────────────────────────────────

#[test]
fn uuid_ids_are_unique_per_mint() {
    assert_ne!(ProjectId::new(), ProjectId::new());
    assert_ne!(SessionId::new(), SessionId::new());
    assert_ne!(InstallationId::new(), InstallationId::new());
}

#[test]
fn nil_is_available_and_distinct_from_a_minted_id() {
    assert_ne!(ProjectId::NIL, ProjectId::new());
    assert_eq!(ProjectId::NIL, ProjectId::NIL);
}

#[test]
fn default_mints_rather_than_returning_nil() {
    // Documented behaviour: `Default` is a fresh id, so a struct that derives
    // Default never silently shares one project identity across instances.
    assert_ne!(ProjectId::default(), ProjectId::NIL);
    assert_ne!(ProjectId::default(), ProjectId::default());
}

// ── Failure cases ─────────────────────────────────────────────────────────────

#[test]
fn invalid_uuid_is_rejected_with_a_typed_error() {
    assert_eq!(
        ProjectId::from_str("not-a-uuid"),
        Err(DomainError::InvalidUuid { kind: "ProjectId" })
    );
    assert_eq!(
        SessionId::from_str(""),
        Err(DomainError::InvalidUuid { kind: "SessionId" })
    );
    // The error names the type that failed, so an API can say which field was bad.
    assert!(ProjectId::from_str("x")
        .unwrap_err()
        .to_string()
        .contains("ProjectId"));
}

#[test]
fn model_id_requires_provider_and_name() {
    assert!(ModelId::parse("").is_err());
    assert!(ModelId::parse("   ").is_err());
    assert!(ModelId::parse("no-slash").is_err());
    assert!(ModelId::parse("/name").is_err(), "empty provider");
    assert!(ModelId::parse("provider/").is_err(), "empty name");

    let ok = ModelId::parse("provider/name").unwrap();
    assert_eq!(ok.provider(), "provider");
    assert_eq!(ok.name(), "name");
}

#[test]
fn model_id_keeps_extra_slashes_in_the_name_segment() {
    // Registries do publish nested names (`hf/org/repo`). Only the first `/`
    // separates provider from name; the rest belongs to the name.
    let id = ModelId::parse("huggingface/BAAI/bge-small-en-v1.5").unwrap();
    assert_eq!(id.provider(), "huggingface");
    assert_eq!(id.name(), "BAAI/bge-small-en-v1.5");
}

#[test]
fn model_id_comparison_is_case_sensitive() {
    // Upstream registries are case-sensitive; normalising here would alias two
    // genuinely different models onto one id.
    assert_ne!(
        ModelId::parse("openai/Text-Embedding-3").unwrap(),
        ModelId::parse("openai/text-embedding-3").unwrap()
    );
}

#[test]
fn snapshot_id_is_preserved_byte_for_byte() {
    // A restore must hand the object store exactly the key it emitted, so
    // SnapshotId must never trim, normalise or re-case anything.
    let key = "  prefix/snapshots/00000001750000000_abc12345.snap  ";
    assert_eq!(SnapshotId::parse(key).unwrap().as_str(), key);

    assert_eq!(
        SnapshotId::parse("   "),
        Err(DomainError::Empty { kind: "SnapshotId" }),
        "whitespace-only keys are still rejected"
    );
}

// ── Nominal typing ────────────────────────────────────────────────────────────

#[test]
fn ids_are_distinct_types_not_interchangeable_aliases() {
    // This is a compile-time property; the test documents it and pins the
    // runtime consequence: two ids built from the same UUID stay separate types.
    let uuid = uuid::Uuid::new_v4();
    let project = ProjectId::from_uuid(uuid);
    let session = SessionId::from_uuid(uuid);

    assert_eq!(project.as_uuid(), session.as_uuid());
    assert_eq!(project.to_string(), session.to_string());
    // `assert_eq!(project, session)` does not compile — that is the point.
}
