// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! Cluster management API — Phase 2.6.
//!
//! HTTP endpoints over the [`ClusterHandle`]'s Raft handle, mounted next to
//! the data-plane router when the node boots in cluster mode:
//!
//! | Method | Path | What |
//! |---|---|---|
//! | GET  | `/v1/cluster/status` | leader, term, indexes, membership |
//! | GET  | `/v1/cluster/health` | 200 when this node sees a leader, 503 otherwise |
//! | POST | `/v1/cluster/add-node` | learner-then-voter membership change |
//! | POST | `/v1/cluster/remove-node` | voter removal |
//!
//! Membership changes are leader-only: a follower answers **403 with the
//! leader's API address** so operators (and scripts) can retry against the
//! right node. Every accepted change goes through Raft itself — the
//! membership entry is committed like any other, so the change is durable
//! and ordered with respect to data writes.
//!
//! Admin-action audit events (`NodeJoined`/`NodeLeft` in the chained log)
//! land in Phase 2.9; these endpoints are the place they'll be emitted.

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::sync::Arc;

use valori_consensus::types::{NodeId, Raft, ValoriNode};
use valori_wire::{AdminEvent, LogEntry};

use crate::events::event_log::EventLogWriter;

/// Shared state for the cluster endpoints.
#[derive(Clone)]
pub struct ClusterApiState {
    pub raft: Arc<Raft>,
    /// Where successful membership changes are recorded as `AdminEvent`s
    /// in the BLAKE3 chain (Phase 2.9). `None` = no audit log configured.
    pub audit: Option<Arc<std::sync::Mutex<EventLogWriter>>>,
}

pub fn cluster_router(
    raft: Arc<Raft>,
    audit: Option<Arc<std::sync::Mutex<EventLogWriter>>>,
) -> Router {
    Router::new()
        .route("/v1/cluster/status", get(status))
        .route("/v1/cluster/health", get(health))
        .route("/v1/cluster/read-index", get(read_index))
        .route("/v1/cluster/role", get(role))
        .route("/v1/cluster/add-node", post(add_node))
        .route("/v1/cluster/remove-node", post(remove_node))
        .with_state(ClusterApiState { raft, audit })
}

/// Record an admin action in the chained audit log. Failures are logged,
/// not surfaced: the membership change has already committed through Raft —
/// the source of truth — and a full audit disk must not unwind it.
fn record_admin(state: &ClusterApiState, event: AdminEvent) {
    if let Some(audit) = &state.audit {
        let mut writer = audit.lock().expect("audit log mutex poisoned");
        if let Err(e) = writer.append(&LogEntry::Admin(event.clone())) {
            tracing::error!("failed to audit admin action {}: {e}", event.describe());
        }
    }
}

// ── Status ────────────────────────────────────────────────────────────────────

#[derive(Serialize)]
struct MemberView {
    id: NodeId,
    raft_addr: String,
    api_addr: String,
    voter: bool,
}

#[derive(Serialize)]
struct StatusView {
    node_id: NodeId,
    current_leader: Option<NodeId>,
    is_leader: bool,
    term: u64,
    last_log_index: Option<u64>,
    last_applied_index: Option<u64>,
    members: Vec<MemberView>,
}

async fn status(State(state): State<ClusterApiState>) -> Json<StatusView> {
    let m = state.raft.metrics().borrow().clone();

    let voters: std::collections::BTreeSet<NodeId> =
        m.membership_config.membership().voter_ids().collect();

    let members = m
        .membership_config
        .nodes()
        .map(|(id, node)| MemberView {
            id: *id,
            raft_addr: node.raft_addr.clone(),
            api_addr: node.api_addr.clone(),
            voter: voters.contains(id),
        })
        .collect();

    Json(StatusView {
        node_id: m.id,
        current_leader: m.current_leader,
        is_leader: m.current_leader == Some(m.id),
        term: m.current_term,
        last_log_index: m.last_log_index,
        last_applied_index: m.last_applied.map(|l| l.index),
        members,
    })
}

// ── Read index (linearizable reads) ─────────────────────────────────────────────
//
// The read-index protocol's leader half. `get_read_log_id` confirms this node
// is still the leader (via a heartbeat round to a quorum) and returns the log
// index a read must observe to be linearizable. A follower calls this, then
// waits until its own applied index reaches `read_index` before serving —
// see `ensure_read_consistency` in cluster_server.rs.

async fn read_index(State(state): State<ClusterApiState>) -> Response {
    match state.raft.get_read_log_id().await {
        Ok((read_log_id, _applied)) => (
            StatusCode::OK,
            Json(serde_json::json!({
                "read_index": read_log_id.map(|l| l.index).unwrap_or(0),
            })),
        )
            .into_response(),
        Err(e) => {
            // Not the leader anymore, or no quorum — name the leader we know so
            // the caller can re-resolve and retry.
            let leader = state.raft.metrics().borrow().current_leader;
            (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(serde_json::json!({
                    "error": "cannot establish read index (not leader or no quorum)",
                    "leader": leader,
                    "detail": format!("{e}"),
                })),
            )
                .into_response()
        }
    }
}

// ── Role (for load-balancer leader routing) ───────────────────────────────────
//
// A load balancer can route POST /records to the leader by health-checking
// this endpoint. On the leader it returns 200 + {"role":"leader"}; on a
// follower it returns 200 + {"role":"follower"}. Both are healthy — the
// distinction lets the LB steer writes without the SDK needing to follow
// redirects for every request.

async fn role(State(state): State<ClusterApiState>) -> Json<serde_json::Value> {
    let m = state.raft.metrics().borrow().clone();
    let is_leader = m.current_leader == Some(m.id);
    Json(serde_json::json!({
        "role": if is_leader { "leader" } else { "follower" },
        "node_id": m.id,
        "current_leader": m.current_leader,
    }))
}

// ── Health ────────────────────────────────────────────────────────────────────

async fn health(State(state): State<ClusterApiState>) -> Response {
    let m = state.raft.metrics().borrow().clone();
    match m.current_leader {
        Some(leader) => (
            StatusCode::OK,
            Json(serde_json::json!({ "status": "ok", "leader": leader })),
        )
            .into_response(),
        None => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({
                "status": "no-leader",
                "detail": "this node currently sees no elected leader",
            })),
        )
            .into_response(),
    }
}

// ── Membership changes ────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct AddNodeRequest {
    node_id: NodeId,
    raft_addr: String,
    #[serde(default)]
    api_addr: String,
}

#[derive(Deserialize)]
struct RemoveNodeRequest {
    node_id: NodeId,
}

/// Maps a leader-only failure to 403 + the leader's API address.
fn leadership_error<E>(raft_err: openraft::error::RaftError<NodeId, E>) -> Response
where
    E: std::fmt::Display,
{
    let body = match &raft_err {
        openraft::error::RaftError::APIError(e) => serde_json::json!({
            "error": "not-leader-or-rejected",
            "detail": e.to_string(),
        }),
        openraft::error::RaftError::Fatal(e) => serde_json::json!({
            "error": "raft-fatal",
            "detail": e.to_string(),
        }),
    };
    (StatusCode::FORBIDDEN, Json(body)).into_response()
}

async fn add_node(
    State(state): State<ClusterApiState>,
    Json(req): Json<AddNodeRequest>,
) -> Response {
    let node = ValoriNode {
        raft_addr: req.raft_addr,
        api_addr: req.api_addr,
    };

    // Step 1: add as learner — it catches up on the log (or receives a
    // snapshot, Phase 2.7) without affecting quorum.
    if let Err(e) = state.raft.add_learner(req.node_id, node.clone(), true).await {
        return leadership_error(e);
    }

    // Step 2: promote to voter. The address is already registered by
    // add_learner, so the change is expressed as the new voter-id set.
    // `retain: false` — the node moves from learner to voter rather than
    // being duplicated in both sets.
    let mut voters: BTreeSet<NodeId> = state
        .raft
        .metrics()
        .borrow()
        .membership_config
        .membership()
        .voter_ids()
        .collect();
    voters.insert(req.node_id);

    match state.raft.change_membership(voters, false).await {
        Ok(resp) => {
            record_admin(
                &state,
                AdminEvent::NodeJoined {
                    node_id: req.node_id,
                    raft_addr: node.raft_addr.clone(),
                    api_addr: node.api_addr.clone(),
                    // All-zeros until RBAC lands (Phase 3): "no auth configured".
                    authorized_by: [0u8; 16],
                },
            );
            (
                StatusCode::OK,
                Json(serde_json::json!({
                    "status": "added",
                    "node_id": req.node_id,
                    "log_index": resp.log_id.index,
                })),
            )
                .into_response()
        }
        Err(e) => leadership_error(e),
    }
}

async fn remove_node(
    State(state): State<ClusterApiState>,
    Json(req): Json<RemoveNodeRequest>,
) -> Response {
    let remaining: BTreeSet<NodeId> = state
        .raft
        .metrics()
        .borrow()
        .membership_config
        .membership()
        .voter_ids()
        .filter(|id| *id != req.node_id)
        .collect();

    if remaining.is_empty() {
        return (
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(serde_json::json!({
                "error": "cannot-remove-last-voter",
                "detail": "removing this node would leave the cluster with no voters",
            })),
        )
            .into_response();
    }

    // `retain: false`: the removed voter is dropped entirely, not demoted
    // to a lingering learner.
    match state.raft.change_membership(remaining, false).await {
        Ok(resp) => {
            record_admin(
                &state,
                AdminEvent::NodeLeft {
                    node_id: req.node_id,
                    authorized_by: [0u8; 16],
                },
            );
            (
                StatusCode::OK,
                Json(serde_json::json!({
                    "status": "removed",
                    "node_id": req.node_id,
                    "log_index": resp.log_id.index,
                })),
            )
                .into_response()
        }
        Err(e) => leadership_error(e),
    }
}
