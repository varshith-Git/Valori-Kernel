// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! Phase 2.1 — the openraft type config is sound and its wire types
//! round-trip. Everything later (log store, state machine, network) is
//! written against these types, so breakage here is breakage everywhere.

use valori_consensus::types::{
    ClientRequest, ClientResponse, Entry, LogId, NodeId, TypeConfig, ValoriNode, Vote,
    CURRENT_SCHEMA_VERSION,
};
use valori_kernel::event::KernelEvent;
use valori_kernel::types::id::RecordId;
use valori_kernel::types::vector::FxpVector;

fn sample_event() -> KernelEvent {
    KernelEvent::InsertRecord {
        id: RecordId(0),
        vector: FxpVector::new_zeros(4),
        metadata: Some(vec![0xAB, 0xCD]),
        tag: 7,
    }
}

const REQ_ID: [u8; 16] = [
    0x01, 0x23, 0x45, 0x67, 0x89, 0xab, 0xcd, 0xef,
    0xfe, 0xdc, 0xba, 0x98, 0x76, 0x54, 0x32, 0x10,
];

// ── ClientRequest: the replicated command ─────────────────────────────────────

#[test]
fn client_request_roundtrips_through_serde_json() {
    let req = ClientRequest {
        event: sample_event(),
        request_id: Some(REQ_ID),
        schema_version: 0,
    };
    let json = serde_json::to_string(&req).unwrap();
    let back: ClientRequest = serde_json::from_str(&json).unwrap();
    assert_eq!(back.request_id, Some(REQ_ID));
    assert_eq!(back.event.event_type(), "InsertRecord");
}

#[test]
fn client_request_without_request_id_decodes_with_default() {
    // Append-only evolution: an old peer that never sends request_id must
    // still decode on a new peer. Simulate by omitting the field entirely.
    let json = serde_json::to_string(&ClientRequest {
        event: KernelEvent::DeleteRecord { id: RecordId(3) },
        request_id: None,
        schema_version: 0,
    })
    .unwrap();
    let stripped = json.replace(",\"request_id\":null", "");
    let back: ClientRequest = serde_json::from_str(&stripped).unwrap();
    assert_eq!(back.request_id, None);
}

#[test]
fn client_response_roundtrips_and_dedup_flag_defaults_false() {
    let resp = ClientResponse {
        log_index: 42,
        state_hash: [9u8; 32],
        deduplicated: false,
        rejected: None,
        allocated_record_id: None,
        allocated_node_id: None,
        allocated_edge_id: None,
    };
    let json = serde_json::to_string(&resp).unwrap();
    // Strip the flag — old responses without it must still decode.
    let stripped = json.replace(",\"deduplicated\":false", "");
    let back: ClientResponse = serde_json::from_str(&stripped).unwrap();
    assert_eq!(back.log_index, 42);
    assert_eq!(back.state_hash, [9u8; 32]);
    assert!(!back.deduplicated);
}

// ── ValoriNode: the membership entry ──────────────────────────────────────────

#[test]
fn valori_node_roundtrips_and_displays_both_addrs() {
    let node = ValoriNode {
        api_addr: "10.0.0.1:3000".into(),
        raft_addr: "10.0.0.1:3100".into(),
    };
    let json = serde_json::to_string(&node).unwrap();
    let back: ValoriNode = serde_json::from_str(&json).unwrap();
    assert_eq!(back, node);

    let shown = format!("{node}");
    assert!(shown.contains("10.0.0.1:3000"), "display must show api addr: {shown}");
    assert!(shown.contains("10.0.0.1:3100"), "display must show raft addr: {shown}");
}

// ── openraft entry/vote types instantiate against the config ─────────────────

#[test]
fn raft_log_entry_for_a_kernel_event_roundtrips() {
    let entry = Entry {
        log_id: LogId::new(openraft::CommittedLeaderId::new(1, 1), 5),
        payload: openraft::EntryPayload::Normal(ClientRequest {
            event: sample_event(),
            request_id: Some(REQ_ID),
            schema_version: 0,
        }),
    };
    let json = serde_json::to_string(&entry).unwrap();
    let back: Entry = serde_json::from_str(&json).unwrap();
    assert_eq!(back.log_id.index, 5);
    match back.payload {
        openraft::EntryPayload::Normal(req) => {
            assert_eq!(req.request_id, Some(REQ_ID));
        }
        other => panic!("expected Normal payload, got {other:?}"),
    }
}

#[test]
fn vote_serializes_for_persistence() {
    // Phase 2.2's log store persists the vote; it must round-trip exactly —
    // a corrupted vote can elect two leaders in one term.
    let vote: Vote = Vote::new(7, 3 as NodeId);
    let json = serde_json::to_string(&vote).unwrap();
    let back: Vote = serde_json::from_str(&json).unwrap();
    assert_eq!(back, vote);
}

// ── Schema version (Phase 3.2) ────────────────────────────────────────────────

#[test]
fn schema_version_field_roundtrips() {
    let req = ClientRequest {
        event: sample_event(),
        request_id: None,
        schema_version: CURRENT_SCHEMA_VERSION,
    };
    let json = serde_json::to_string(&req).unwrap();
    let back: ClientRequest = serde_json::from_str(&json).unwrap();
    assert_eq!(back.schema_version, CURRENT_SCHEMA_VERSION);
}

#[test]
fn schema_version_defaults_to_zero_when_field_absent() {
    // Old nodes that pre-date Phase 3.2 never wrote schema_version.
    // They must decode without error and the field must default to 0.
    let req = ClientRequest {
        event: KernelEvent::DeleteRecord { id: RecordId(99) },
        request_id: None,
        schema_version: 0,
    };
    let json = serde_json::to_string(&req).unwrap();
    // Strip the schema_version field entirely to simulate an old peer.
    let stripped = json
        .replace(",\"schema_version\":0", "")
        .replace("\"schema_version\":0,", "");
    let back: ClientRequest = serde_json::from_str(&stripped).unwrap();
    assert_eq!(back.schema_version, 0, "missing schema_version must default to 0");
}

#[test]
fn current_schema_version_is_zero() {
    // This test encodes the current contract. When you bump CURRENT_SCHEMA_VERSION,
    // update this assertion AND add a compatibility-matrix entry in docs/COMPATIBILITY.md.
    assert_eq!(CURRENT_SCHEMA_VERSION, 0);
}

#[test]
fn raft_handle_type_is_instantiable() {
    // Compile-time proof: Raft<TypeConfig> is a valid type. (Constructing a
    // live instance needs the Phase 2.2 store and 2.4 network.)
    fn _assert_type<T>() {}
    _assert_type::<openraft::Raft<TypeConfig>>();
}
