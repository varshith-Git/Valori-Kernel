// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! Linearizable reads via the read-index protocol.
//!
//! Brings up a real 2-node cluster (gRPC consensus + HTTP data plane on real
//! ports), writes through the leader, and proves a *follower* serving a
//! linearizable search reflects that write — i.e. the follower ran the
//! read-index round trip to the leader and waited for its own apply to catch
//! up before answering. A `local` read on the same node is the contrast.

use std::time::Duration;

use valori_consensus::types::{NodeId, ValoriNode};
use valori_consensus::{ClientRequest, NullAuditSink};
use valori_kernel::event::KernelEvent;
use valori_kernel::types::scalar::FxpScalar;
use valori_kernel::types::vector::FxpVector;
use valori_node::cluster::{bootstrap_cluster, ClusterConfig, ClusterHandle};
use valori_node::cluster_server::serve_cluster_api;

struct Node {
    handle: ClusterHandle,
    api_addr: String,
    _http_task: tokio::task::JoinHandle<()>,
}

async fn boot(id: NodeId) -> Node {
    let cfg = ClusterConfig {
        node_id: id,
        raft_bind: "127.0.0.1:0".into(),
        // Real membership is installed by the explicit initialize() below; the
        // per-node config only needs to name itself.
        members: [(id, ValoriNode::default())].into_iter().collect(),
        init: false,
        raft_log_path: None,
        tls: None,
    };
    let handle = bootstrap_cluster(&cfg, Box::new(NullAuditSink)).await.unwrap();
    let (api, task) = serve_cluster_api(&handle, "127.0.0.1:0", None).await.unwrap();
    Node { handle, api_addr: api.to_string(), _http_task: task }
}

/// Stand up a 2-node cluster and return the nodes once a leader exists.
async fn two_node_cluster() -> Vec<Node> {
    let n1 = boot(1).await;
    let n2 = boot(2).await;

    // All gRPC + HTTP servers are listening now, so the first election succeeds
    // without retries. Build membership from the actually-bound addresses.
    let members: std::collections::BTreeMap<NodeId, ValoriNode> = [
        (1, ValoriNode { api_addr: n1.api_addr.clone(), raft_addr: n1.handle.raft_addr.to_string() }),
        (2, ValoriNode { api_addr: n2.api_addr.clone(), raft_addr: n2.handle.raft_addr.to_string() }),
    ]
    .into_iter()
    .collect();
    n1.handle.raft.initialize(members).await.unwrap();

    // Wait for a leader to emerge on either node.
    for n in [&n1, &n2] {
        n.handle
            .raft
            .wait(Some(Duration::from_secs(10)))
            .metrics(|m| m.current_leader.is_some(), "leader elected")
            .await
            .unwrap();
    }
    vec![n1, n2]
}

fn leader_id(nodes: &[Node]) -> NodeId {
    nodes[0].handle.raft.metrics().borrow().current_leader.expect("a leader")
}

async fn search(api_addr: &str, query: Vec<f32>, consistency: &str) -> serde_json::Value {
    reqwest::Client::new()
        .post(format!("http://{api_addr}/search"))
        .json(&serde_json::json!({ "query": query, "k": 1, "consistency": consistency }))
        .timeout(Duration::from_secs(10))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn follower_linearizable_read_reflects_a_leader_write() {
    let nodes = two_node_cluster().await;
    let leader = leader_id(&nodes);

    // Pick the follower (the node that is not the leader).
    let follower = nodes.iter().find(|n| n.handle.raft.metrics().borrow().id != leader).unwrap();
    let leader_node = nodes.iter().find(|n| n.handle.raft.metrics().borrow().id == leader).unwrap();

    // Write through the leader's Raft handle (AutoInsertRecord → id 0).
    let resp = leader_node
        .handle
        .raft
        .client_write(ClientRequest {
            event: KernelEvent::AutoInsertRecord {
                vector: FxpVector { data: vec![FxpScalar(100), FxpScalar(200), FxpScalar(300)] },
                metadata: None,
                tag: 0,
            },
            schema_version: 0,
            request_id: None,
        })
        .await
        .unwrap();
    assert_eq!(resp.data.allocated_record_id, Some(0));

    // A linearizable search on the FOLLOWER must reflect the write: the handler
    // runs the read-index round trip to the leader and waits to catch up.
    let body = search(&follower.api_addr, vec![1.0, 2.0, 3.0], "linearizable").await;
    let results = body["results"].as_array().expect("results array");
    assert_eq!(results.len(), 1, "follower linearizable read missed the write: {body}");
    assert_eq!(results[0]["id"], 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn read_index_endpoint_serves_on_leader_and_redirects_intent_on_follower() {
    let nodes = two_node_cluster().await;
    let leader = leader_id(&nodes);
    let leader_node = nodes.iter().find(|n| n.handle.raft.metrics().borrow().id == leader).unwrap();
    let follower = nodes.iter().find(|n| n.handle.raft.metrics().borrow().id != leader).unwrap();

    let client = reqwest::Client::new();

    // Leader answers with a read index.
    let on_leader = client
        .get(format!("http://{}/v1/cluster/read-index", leader_node.api_addr))
        .send()
        .await
        .unwrap();
    assert_eq!(on_leader.status(), reqwest::StatusCode::OK);
    let v: serde_json::Value = on_leader.json().await.unwrap();
    assert!(v.get("read_index").and_then(|x| x.as_u64()).is_some(), "leader read-index: {v}");

    // Follower cannot establish a read index itself — it answers 503 and names
    // the leader so the caller re-resolves.
    let on_follower = client
        .get(format!("http://{}/v1/cluster/read-index", follower.api_addr))
        .send()
        .await
        .unwrap();
    assert_eq!(on_follower.status(), reqwest::StatusCode::SERVICE_UNAVAILABLE);
    let v: serde_json::Value = on_follower.json().await.unwrap();
    assert_eq!(v["leader"].as_u64(), Some(leader));
}
