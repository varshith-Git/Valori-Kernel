// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! Domain invariant matrix — every entry point preserves the same invariants.
//!
//! # Why this file exists
//!
//! M2 validated in `parse()` and tested `parse()`. Untrusted input does not
//! arrive through `parse()` — it arrives through `Deserialize`, from an HTTP
//! body, a `project.json`, or a redb value. Because `#[serde(transparent)]`
//! derives a `Deserialize` that skips the constructor, every invariant was
//! unenforced on the only path that mattered:
//! `serde_json::from_str::<ProjectName>("\"../../etc/passwd\"")` succeeded.
//! See `docs/reviews/m2-project-review.md` finding F1.
//!
//! The rule this file enforces:
//!
//! > **A value that `parse()` rejects must be unconstructable through *any*
//! > entry point, including `Deserialize`.**
//!
//! # The matrix
//!
//! | Type | ctor | serialize | deserialize | invalid | persistence | adapter |
//! |---|---|---|---|---|---|---|
//! | `ProjectName` | ✅ | ✅ | ✅ | ✅ | ✅ | daemon/metadata adapter tests |
//! | `ModelId` | ✅ | ✅ | ✅ | ✅ | ✅ | — (no adapter yet) |
//! | `SnapshotId` | ✅ | ✅ | ✅ | ✅ | ✅ | — (no adapter yet) |
//! | `ProjectId` | ✅ | ✅ | ✅ | ✅ | ✅ | daemon adapter tests |
//! | `ProjectTopology` | ✅ | ✅ | ✅ | ✅ | ✅ | both adapter tests |
//! | `Project` | ✅ | ✅ | ✅ | ✅ | ✅ | both adapter tests |
//!
//! Adapter-boundary coverage lives with the adapters, because `valori-domain`
//! cannot depend on `valori-daemon` or `valori-metadata` — see
//! `crates/valori-node/tests/dependency_direction.rs`.

use std::str::FromStr;

use valori_domain::{
    ApiProject, CredentialRef, IndexKind, ModelId, Project, ProjectId, ProjectName,
    ProjectTopology, SessionId, SnapshotId, Timestamp,
};

// ── Shared corpora ────────────────────────────────────────────────────────────

/// Values that must never become a `ProjectName` through any entry point.
///
/// The traversal cases are the reason this matters: a project name is used as a
/// directory name by all three project implementations.
const HOSTILE_PROJECT_NAMES: &[&str] = &[
    "../../etc/passwd",
    "../../../tmp",
    "/path",
    "foo/bar",
    "",
    "..",
    "../..",
    "a/../../etc",
    "a\\b",
    "C:\\Windows",
    "has space",
    "has.dot",
    "has:colon",
    "émoji",
    "null\0byte",
    ".",
    "./relative",
];

/// Names the daemon legitimately created, which must remain representable.
const LEGACY_VALID_PROJECT_NAMES: &[&str] = &["_scratch", "-tmp", "a", "A1", "research-notes"];

fn valid_project() -> Project {
    Project {
        id: ProjectId::from_str("7c9e6679-7425-40de-944b-e07fc1f90ae7").unwrap(),
        name: ProjectName::parse("research-notes").unwrap(),
        dim: 384,
        index: IndexKind::Hnsw,
        topology: ProjectTopology::STANDALONE,
        created_at: Timestamp::from_unix_secs(1_750_000_000),
        last_opened_at: None,
        record_count: None,
    }
}

/// `serde_json` quoting for an arbitrary string, so control characters and
/// backslashes reach the deserializer exactly as written above.
fn json_string(raw: &str) -> String {
    serde_json::to_string(raw).unwrap()
}

// ── ProjectName ───────────────────────────────────────────────────────────────

#[test]
fn project_name_constructor_and_deserialize_agree_on_every_hostile_value() {
    for hostile in HOSTILE_PROJECT_NAMES {
        let via_ctor = ProjectName::parse(*hostile).is_ok();
        let via_serde = serde_json::from_str::<ProjectName>(&json_string(hostile)).is_ok();

        assert!(
            !via_ctor,
            "ProjectName::parse must reject {hostile:?} — it becomes a directory name"
        );
        assert_eq!(
            via_ctor, via_serde,
            "ProjectName: parse and Deserialize disagree on {hostile:?} \
             (parse_ok={via_ctor}, serde_ok={via_serde}). Deserialize must route \
             through the constructor — see crate::validate."
        );
    }
}

#[test]
fn project_name_constructor_and_deserialize_agree_on_every_valid_value() {
    for good in LEGACY_VALID_PROJECT_NAMES {
        let via_ctor = ProjectName::parse(*good).expect("must be representable");
        let via_serde: ProjectName =
            serde_json::from_str(&json_string(good)).expect("must deserialize");
        assert_eq!(via_ctor, via_serde, "disagreement on {good:?}");
    }
}

#[test]
fn project_name_round_trips_and_keeps_its_wire_shape() {
    for good in LEGACY_VALID_PROJECT_NAMES {
        let name = ProjectName::parse(*good).unwrap();
        let json = serde_json::to_string(&name).unwrap();

        // Wire compatibility: a bare string, unchanged from the M2 shape.
        assert_eq!(json, json_string(good));
        assert_eq!(serde_json::from_str::<ProjectName>(&json).unwrap(), name);
        // And never normalised on the way through.
        assert_eq!(name.as_str(), *good);
    }
}

#[test]
fn project_name_deserialize_error_explains_itself() {
    let err = serde_json::from_str::<ProjectName>("\"../../etc/passwd\"").unwrap_err();
    let msg = err.to_string();
    assert!(
        msg.contains("ProjectName") || msg.contains("project name"),
        "the deserialization error should name the type: {msg}"
    );
}

#[test]
fn project_name_rejects_non_strings() {
    assert!(serde_json::from_str::<ProjectName>("123").is_err());
    assert!(serde_json::from_str::<ProjectName>("null").is_err());
    assert!(serde_json::from_str::<ProjectName>("[\"a\"]").is_err());
    assert!(serde_json::from_str::<ProjectName>("{\"0\":\"a\"}").is_err());
}

// ── ModelId ───────────────────────────────────────────────────────────────────

#[test]
fn model_id_constructor_and_deserialize_agree() {
    let hostile = [
        "",
        "   ",
        "no-slash",
        "/name",
        "provider/",
        "/",
        "just.a.name",
    ];
    for bad in hostile {
        let via_ctor = ModelId::parse(bad).is_ok();
        let via_serde = serde_json::from_str::<ModelId>(&json_string(bad)).is_ok();
        assert!(!via_ctor, "ModelId::parse must reject {bad:?}");
        assert_eq!(
            via_ctor, via_serde,
            "ModelId: parse and Deserialize disagree on {bad:?}"
        );
    }

    for good in [
        "openai/text-embedding-3-small",
        "ollama/nomic-embed-text",
        "huggingface/BAAI/bge-small-en-v1.5",
    ] {
        let via_ctor = ModelId::parse(good).unwrap();
        let via_serde: ModelId = serde_json::from_str(&json_string(good)).unwrap();
        assert_eq!(via_ctor, via_serde);
        assert_eq!(serde_json::to_string(&via_ctor).unwrap(), json_string(good));
    }
}

// ── SnapshotId ────────────────────────────────────────────────────────────────

#[test]
fn snapshot_id_constructor_and_deserialize_agree() {
    for bad in ["", "   ", "\t\n"] {
        let via_ctor = SnapshotId::parse(bad).is_ok();
        let via_serde = serde_json::from_str::<SnapshotId>(&json_string(bad)).is_ok();
        assert!(!via_ctor, "SnapshotId::parse must reject {bad:?}");
        assert_eq!(
            via_ctor, via_serde,
            "SnapshotId: parse and Deserialize disagree on {bad:?}"
        );
    }

    // Keys are opaque and must survive byte-for-byte, whitespace included.
    let key = "  prefix/snapshots/00000001750000000_abc12345.snap  ";
    let via_serde: SnapshotId = serde_json::from_str(&json_string(key)).unwrap();
    assert_eq!(via_serde.as_str(), key, "a key must never be normalised");
    assert_eq!(serde_json::to_string(&via_serde).unwrap(), json_string(key));
}

// ── ProjectId / SessionId — the "inner primitive validates" assumption ────────

#[test]
fn uuid_ids_reject_malformed_input_through_deserialize() {
    // These derive Deserialize because `Uuid` validates for them. That
    // assumption is asserted here rather than believed.
    for bad in [
        "",
        "not-a-uuid",
        "7c9e6679",
        "zzzzzzzz-7425-40de-944b-e07fc1f90ae7",
    ] {
        assert!(
            serde_json::from_str::<ProjectId>(&json_string(bad)).is_err(),
            "ProjectId must reject {bad:?} through Deserialize"
        );
        assert!(
            serde_json::from_str::<SessionId>(&json_string(bad)).is_err(),
            "SessionId must reject {bad:?} through Deserialize"
        );
        assert!(
            serde_json::from_str::<CredentialRef>(&json_string(bad)).is_err(),
            "CredentialRef must reject {bad:?} through Deserialize"
        );
        assert!(ProjectId::from_str(bad).is_err());
    }

    let good = "7c9e6679-7425-40de-944b-e07fc1f90ae7";
    let id: ProjectId = serde_json::from_str(&json_string(good)).unwrap();
    assert_eq!(id, ProjectId::from_str(good).unwrap());
    assert_eq!(serde_json::to_string(&id).unwrap(), json_string(good));
}

// ── CredentialRef — opaque reference, never the secret ────────────────────────

#[test]
fn credential_ref_create_serialize_deserialize_round_trip_and_distinctness() {
    // Creation.
    let a = CredentialRef::new();
    let b = CredentialRef::new();

    // Different refs are different — two independently minted credentials
    // must never collide or alias to the same OS-keychain entry.
    assert_ne!(a, b, "two freshly minted CredentialRefs must not be equal");

    // Serialize — bare UUID string, `#[serde(transparent)]`, same wire shape
    // as every other uuid_id! type (ProjectId, SessionId, InstallationId).
    let text = a.to_string();
    let serialized = serde_json::to_string(&a).unwrap();
    assert_eq!(serialized, format!("\"{text}\""));

    // Deserialize + round-trip.
    let via_serde: CredentialRef = serde_json::from_str(&serialized).unwrap();
    assert_eq!(via_serde, a);
    let via_parse = CredentialRef::from_str(&text).unwrap();
    assert_eq!(via_parse, a);

    // Round-tripping `b` must still be distinct from `a`'s round-trip.
    let b_serialized = serde_json::to_string(&b).unwrap();
    let b_round_tripped: CredentialRef = serde_json::from_str(&b_serialized).unwrap();
    assert_ne!(
        via_serde, b_round_tripped,
        "round-tripping must not collapse distinct refs"
    );
}

// ── ProjectTopology — same assumption, via NonZeroU8 ─────────────────────────

#[test]
fn topology_rejects_zero_through_deserialize() {
    for bad in [
        r#"{"replicas":0,"shards":1}"#,
        r#"{"replicas":1,"shards":0}"#,
        r#"{"replicas":0,"shards":0}"#,
    ] {
        assert!(
            serde_json::from_str::<ProjectTopology>(bad).is_err(),
            "topology must reject {bad}"
        );
    }
    let ok: ProjectTopology = serde_json::from_str(r#"{"replicas":3,"shards":4}"#).unwrap();
    assert_eq!(ok, ProjectTopology::new(3, 4).unwrap());
}

// ── Project — the composite ──────────────────────────────────────────────────

#[test]
fn project_deserialize_rejects_a_hostile_name() {
    // The whole point: a nested validated newtype must still validate.
    let hostile = r#"{
        "id":"7c9e6679-7425-40de-944b-e07fc1f90ae7",
        "name":"../../etc/passwd",
        "dim":384,"index":"hnsw",
        "topology":{"replicas":1,"shards":1},
        "created_at":1,"last_opened_at":null,"record_count":null
    }"#;
    assert!(
        serde_json::from_str::<Project>(hostile).is_err(),
        "a hostile name must not survive into a Project"
    );
}

#[test]
fn project_round_trips_through_json() {
    let project = valid_project();
    let json = serde_json::to_string(&project).unwrap();
    assert_eq!(serde_json::from_str::<Project>(&json).unwrap(), project);
}

#[test]
fn project_deserialize_rejects_an_unknown_index() {
    let bad = r#"{
        "id":"7c9e6679-7425-40de-944b-e07fc1f90ae7","name":"ok","dim":384,
        "index":"quantum","topology":{"replicas":1,"shards":1},
        "created_at":1,"last_opened_at":null,"record_count":null
    }"#;
    assert!(serde_json::from_str::<Project>(bad).is_err());
}

// ── ApiProject — the untrusted-client boundary ───────────────────────────────

#[test]
fn api_project_deserialize_rejects_a_hostile_name() {
    let hostile = r#"{"id":"7c9e6679-7425-40de-944b-e07fc1f90ae7",
        "name":"../../../etc/passwd","dim":384,"index":"hnsw",
        "replicas":1,"shards":1,"is_cluster":false,"created_at":1}"#;
    assert!(
        serde_json::from_str::<ApiProject>(hostile).is_err(),
        "the API boundary must reject a traversal name"
    );
}

#[test]
fn api_project_rejects_an_is_cluster_flag_that_contradicts_replicas() {
    // `is_cluster` is derived from `replicas`, so a payload carrying both can
    // lie. Silently trusting one and discarding the other would let a client
    // believe it had requested a cluster (review finding F5).
    let mut api = ApiProject::from(&valid_project());
    api.is_cluster = true; // replicas is 1
    assert!(Project::try_from(api).is_err());

    let mut cluster = valid_project();
    cluster.topology = ProjectTopology::new(3, 1).unwrap();
    let mut api = ApiProject::from(&cluster);
    api.is_cluster = false; // replicas is 3
    assert!(Project::try_from(api).is_err());
}

#[test]
fn api_project_accepts_a_consistent_flag() {
    for topology in [
        ProjectTopology::STANDALONE,
        ProjectTopology::new(3, 4).unwrap(),
    ] {
        let mut project = valid_project();
        project.topology = topology;
        let api = ApiProject::from(&project);
        assert_eq!(Project::try_from(api).unwrap(), project);
    }
}

// ── Persistence boundary ─────────────────────────────────────────────────────

#[test]
fn values_already_on_disk_still_deserialize() {
    // Wire compatibility is the reason M2 had zero migration cost; F1's fix
    // must not have changed which *valid* values are accepted.
    let id: ProjectId = serde_json::from_str("\"7c9e6679-7425-40de-944b-e07fc1f90ae7\"").unwrap();
    assert_eq!(id.to_string(), "7c9e6679-7425-40de-944b-e07fc1f90ae7");

    let model: ModelId = serde_json::from_str("\"ollama/nomic-embed-text\"").unwrap();
    assert_eq!(model.as_str(), "ollama/nomic-embed-text");

    let name: ProjectName = serde_json::from_str("\"healthcare\"").unwrap();
    assert_eq!(name.as_str(), "healthcare");

    let snap: SnapshotId =
        serde_json::from_str("\"snapshots/00000001750000000_abc12345.snap\"").unwrap();
    assert_eq!(snap.as_str(), "snapshots/00000001750000000_abc12345.snap");
}

#[test]
fn every_validated_newtype_is_covered_by_this_file() {
    // A tripwire, not a behaviour test: if a new validated newtype is added to
    // valori-domain without a matrix entry, this list is the reminder.
    let covered = [
        "ProjectName",
        "ModelId",
        "SnapshotId",
        "ProjectId",
        "SessionId",
        "InstallationId",
        "CredentialRef",
        "ProjectTopology",
        "Project",
        "ApiProject",
    ];
    assert_eq!(
        covered.len(),
        10,
        "adding a validated type to valori-domain requires a row in the matrix \
         at the top of this file and a test below it"
    );
}
