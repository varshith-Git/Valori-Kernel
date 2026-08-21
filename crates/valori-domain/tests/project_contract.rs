// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! Contract for the canonical Project model and its API representation.
//!
//! The assertions about `ApiProject`'s JSON shape are a **wire contract**.
//! Changing one is a breaking API change governed by `COMPATIBILITY.md`, not a
//! test to be updated.

use std::str::FromStr;

use valori_domain::{
    ApiProject, DomainError, IndexKind, LocalProject, Project, ProjectId, ProjectName,
    ProjectTopology, Timestamp,
};

fn project() -> Project {
    Project {
        id: ProjectId::from_str("7c9e6679-7425-40de-944b-e07fc1f90ae7").unwrap(),
        name: ProjectName::parse("research-notes").unwrap(),
        topology: ProjectTopology::STANDALONE,
        created_at: Timestamp::from_unix_secs(1_750_000_000),
        last_opened_at: None,
        record_count: None,
    }
}

// ── ProjectName ───────────────────────────────────────────────────────────────

#[test]
fn project_name_accepts_every_name_the_daemon_accepts() {
    // The compatibility contract: ProjectName must be able to represent every
    // project the daemon has legitimately created, or such projects vanish from
    // listings (review finding F2). These three were found during that review.
    for existing in ["_scratch", "-tmp", &"a".repeat(64)] {
        assert!(
            ProjectName::parse(existing).is_ok(),
            "must represent the daemon-created name {existing:?}"
        );
    }

    for ok in ["a", "A1", "research-notes", "my_project_2"] {
        assert!(ProjectName::parse(ok).is_ok(), "should accept {ok:?}");
    }
}

#[test]
fn project_name_rejects_what_the_daemon_also_rejects() {
    // The character rule is what makes the name safe as a directory name.
    for bad in [
        "",
        "has space",
        "has/slash",
        "has.dot",
        "../escape",
        "émoji",
        "back\\slash",
    ] {
        assert!(ProjectName::parse(bad).is_err(), "should reject {bad:?}");
    }

    assert!(
        ProjectName::parse("a".repeat(65)).is_err(),
        "65 bytes exceeds the daemon's 64-character contract"
    );
}

#[test]
fn project_name_rejects_path_traversal() {
    // The name is used as a directory name by all three implementations, so
    // this is a security property, not a formatting preference.
    for attack in [
        "..",
        "../..",
        "a/../../etc",
        "a\\b",
        "/abs",
        "../../etc/passwd",
        "../../../tmp",
    ] {
        assert!(
            ProjectName::parse(attack).is_err(),
            "must reject {attack:?}"
        );
    }
}

#[test]
fn new_project_policy_is_stricter_than_representability() {
    // Names that exist and must remain representable, but that the creation
    // policy declines for NEW projects. Separating these is the F2 fix: the
    // value object describes what exists, the policy constrains what is new.
    for existing_only in ["_scratch", "-tmp"] {
        let name = ProjectName::parse(existing_only).expect("must be representable");
        assert!(
            name.check_new_project_policy().is_err(),
            "{existing_only:?} should be refused for a NEW project"
        );
    }

    let long = ProjectName::parse("a".repeat(64)).unwrap();
    assert!(
        long.check_new_project_policy().is_err(),
        "64 characters exceeds the 63-character new-project limit"
    );

    let sixty_three = ProjectName::parse("a".repeat(63)).unwrap();
    assert!(sixty_three.check_new_project_policy().is_ok());

    for fresh in ["a", "A1", "research-notes", "my_project_2"] {
        let name = ProjectName::parse(fresh).unwrap();
        assert!(
            name.check_new_project_policy().is_ok(),
            "{fresh:?} should be allowed for a new project"
        );
    }
}

#[test]
fn new_project_policy_matches_the_typescript_validator() {
    // ui/src/lib/server/projects.ts::isValidName
    //   ^[a-zA-Z0-9](?:[a-zA-Z0-9_-]{0,62})$
    let ts_accepts = |n: &str| -> bool {
        !n.is_empty()
            && n.len() <= 63
            && n.chars().next().is_some_and(|c| c.is_ascii_alphanumeric())
            && n.chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    };

    for candidate in [
        "a",
        "A1",
        "research-notes",
        "my_project_2",
        "_scratch",
        "-tmp",
        "has space",
        "..",
    ] {
        let domain_accepts = ProjectName::parse(candidate)
            .map(|n| n.check_new_project_policy().is_ok())
            .unwrap_or(false);
        assert_eq!(
            domain_accepts,
            ts_accepts(candidate),
            "creation policy disagrees with the UI validator on {candidate:?}"
        );
    }
}

// ── Topology ──────────────────────────────────────────────────────────────────

#[test]
fn topology_makes_illegal_states_unrepresentable() {
    assert!(ProjectTopology::new(0, 1).is_err(), "zero replicas");
    assert!(ProjectTopology::new(1, 0).is_err(), "zero shards");

    let standalone = ProjectTopology::new(1, 1).unwrap();
    assert!(!standalone.is_cluster());

    let cluster = ProjectTopology::new(3, 4).unwrap();
    assert!(cluster.is_cluster());
}

#[test]
fn topology_does_not_restrict_replicas_to_one_or_three() {
    // The TypeScript union allows only 1 | 3. RFC-0007 does not, and the domain
    // model must not make a legitimate 5-node cluster unrepresentable.
    assert!(ProjectTopology::new(5, 1).unwrap().is_cluster());
}

#[test]
fn cluster_mode_is_derived_never_stored() {
    // There is no `mode` field to contradict `replicas` — that is the fix for
    // metadata::Project's mode/node_count divergence.
    let t = ProjectTopology::new(3, 1).unwrap();
    assert_eq!(t.is_cluster(), t.replicas.get() > 1);
}

// ── IndexKind ─────────────────────────────────────────────────────────────────

#[test]
fn index_kind_parses_every_form_in_use_today() {
    for (input, expected) in [
        ("brute", IndexKind::Brute),
        ("bruteforce", IndexKind::Brute),
        ("hnsw", IndexKind::Hnsw),
        ("ivf", IndexKind::Ivf),
        ("bq", IndexKind::Bq),
        ("auto", IndexKind::Auto),
        ("mstg", IndexKind::Auto),
    ] {
        assert_eq!(
            IndexKind::from_str(input).unwrap(),
            expected,
            "`{input}` is accepted by valori_node::config and valori_metadata"
        );
    }

    assert!(matches!(
        IndexKind::from_str("quantum"),
        Err(DomainError::UnknownIndexKind { .. })
    ));
}

#[test]
fn index_kind_renders_the_canonical_tag_not_the_alias() {
    assert_eq!(IndexKind::from_str("mstg").unwrap().as_str(), "auto");
    assert_eq!(IndexKind::from_str("bruteforce").unwrap().as_str(), "brute");
}

#[test]
fn index_kind_serializes_lowercase() {
    assert_eq!(serde_json::to_string(&IndexKind::Hnsw).unwrap(), "\"hnsw\"");
    assert_eq!(serde_json::to_string(&IndexKind::Bq).unwrap(), "\"bq\"");
}

// ── ApiProject wire shape ─────────────────────────────────────────────────────

#[test]
fn api_project_wire_shape_is_pinned() {
    let api = ApiProject::from(&project());
    let json: serde_json::Value = serde_json::to_value(&api).unwrap();

    assert_eq!(json["id"], "7c9e6679-7425-40de-944b-e07fc1f90ae7");
    assert_eq!(json["name"], "research-notes");
    assert!(
        json.get("dim").is_none() && json.get("index").is_none(),
        "Project/ApiProject must never carry dim/index again — see \
         docs/phases/phase-collection-index-lifecycle.md"
    );
    assert_eq!(json["replicas"], 1);
    assert_eq!(json["shards"], 1);
    assert_eq!(json["is_cluster"], false);
    assert_eq!(
        json["created_at"], 1_750_000_000_u64,
        "unix seconds, not an ISO string"
    );

    // Absent rather than null, so clients need no null handling.
    assert!(json.get("last_opened_at").is_none());
    assert!(json.get("record_count").is_none());
}

#[test]
fn api_project_never_leaks_a_path_or_a_secret() {
    let api = ApiProject::from(&project());
    let json = serde_json::to_string(&api).unwrap();

    for forbidden in [
        "dir",
        "root",
        "path",
        "api_key",
        "apiKey",
        "workspace",
        "port",
    ] {
        assert!(
            !json.contains(forbidden),
            "ApiProject must not carry `{forbidden}`: {json}"
        );
    }
}

#[test]
fn api_project_round_trips_through_the_domain_model() {
    let original = project();
    let api = ApiProject::from(&original);
    let back = Project::try_from(api).unwrap();
    assert_eq!(original, back);
}

#[test]
fn api_project_with_optional_fields_round_trips() {
    let mut p = project();
    p.last_opened_at = Some(Timestamp::from_unix_secs(1_750_000_500));
    p.record_count = Some(1234);
    p.topology = ProjectTopology::new(3, 4).unwrap();

    let api = ApiProject::from(&p);
    assert!(api.is_cluster);
    assert_eq!(api.replicas, 3);
    assert_eq!(api.shards, 4);

    assert_eq!(Project::try_from(api).unwrap(), p);
}

#[test]
fn api_project_rejects_a_zero_topology_from_an_untrusted_client() {
    let mut api = ApiProject::from(&project());
    api.replicas = 0;
    assert!(
        Project::try_from(api).is_err(),
        "validation happens at the boundary, not three layers up"
    );
}

// ── Identity vs location ──────────────────────────────────────────────────────

#[test]
fn moving_a_project_does_not_change_its_identity() {
    let p = project();
    let before = LocalProject::new(p.clone(), "/home/a/.valori/projects/research-notes");
    let after = LocalProject::new(p.clone(), "/mnt/backup/restored/research-notes");

    assert_eq!(before.id(), after.id(), "the path is not the identity");
    assert_ne!(before.root(), after.root());
    assert_eq!(before.project, after.project);
}

#[test]
fn renaming_a_project_does_not_change_its_identity() {
    let original = project();
    let mut renamed = original.clone();
    renamed.name = ProjectName::parse("archived-notes").unwrap();

    assert_eq!(
        original.id, renamed.id,
        "the display name is not the identity"
    );
    assert_ne!(original.name, renamed.name);
}

#[test]
fn two_projects_may_share_a_name_but_never_an_identity() {
    let mut a = project();
    let mut b = project();
    a.id = ProjectId::new();
    b.id = ProjectId::new();
    assert_eq!(a.name, b.name);
    assert_ne!(a.id, b.id);
}
