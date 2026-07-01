// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! Phase 2.10b — mutual TLS on the Raft channel.
//!
//! The Phase 1.6 contract: a peer that cannot present a certificate signed
//! by THIS cluster's CA is refused at the TLS handshake — it never reaches
//! the Raft layer. Two clusters sharing a network (or an attacker with a
//! self-signed cert) cannot exchange a single Raft RPC.
//!
//! Certificates are generated in-test with rcgen: one cluster CA, leaf
//! certs per node, and a "rogue" CA for the negative case.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use openraft::{Config, Raft};
use rcgen::{BasicConstraints, CertificateParams, IsCa, KeyPair};

use valori_consensus::types::{ClientRequest, NodeId, ShardId, TypeConfig, ValoriNode};
use valori_consensus::{
    serve_raft_tls_single, RaftTlsConfig, ValoriLogStore, ValoriNetworkFactory, ValoriStateMachine,
};
use valori_kernel::event::KernelEvent;
use valori_kernel::types::id::RecordId;
use valori_kernel::types::vector::FxpVector;

const DOMAIN: &str = "valori-cluster.internal";

struct TestCa {
    cert: rcgen::Certificate,
    key: KeyPair,
}

fn make_ca(name: &str) -> TestCa {
    let mut params = CertificateParams::new(vec![]).unwrap();
    params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    params
        .distinguished_name
        .push(rcgen::DnType::CommonName, name);
    let key = KeyPair::generate().unwrap();
    let cert = params.self_signed(&key).unwrap();
    TestCa { cert, key }
}

/// A leaf certificate for one node, signed by `ca`, valid for [`DOMAIN`].
fn make_leaf(ca: &TestCa) -> (String, String) {
    let params = CertificateParams::new(vec![DOMAIN.to_string()]).unwrap();
    let key = KeyPair::generate().unwrap();
    let cert = params.signed_by(&key, &ca.cert, &ca.key).unwrap();
    (cert.pem(), key.serialize_pem())
}

fn tls_config(ca: &TestCa, leaf: &(String, String)) -> RaftTlsConfig {
    RaftTlsConfig {
        ca_pem: ca.cert.pem().into_bytes(),
        cert_pem: leaf.0.clone().into_bytes(),
        key_pem: leaf.1.clone().into_bytes(),
        domain: DOMAIN.to_string(),
    }
}

struct TestNode {
    raft: Raft<TypeConfig>,
    sm: ValoriStateMachine,
    node: ValoriNode,
}

async fn spawn_tls_node(id: NodeId, tls: RaftTlsConfig) -> TestNode {
    let config = Arc::new(
        Config {
            heartbeat_interval: 100,
            election_timeout_min: 250,
            election_timeout_max: 500,
            ..Default::default()
        }
        .validate()
        .unwrap(),
    );
    let sm = ValoriStateMachine::default();
    let raft = Raft::new(
        id,
        config,
        ValoriNetworkFactory::with_tls(ShardId(0), tls.clone()),
        ValoriLogStore::new(),
        sm.clone(),
    )
    .await
    .unwrap();
    let (addr, _task) = serve_raft_tls_single(raft.clone(), "127.0.0.1:0", tls).await.unwrap();
    TestNode {
        raft,
        sm,
        node: ValoriNode {
            api_addr: String::new(),
            raft_addr: addr.to_string(),
        },
    }
}

fn insert(id: u32) -> ClientRequest {
    ClientRequest {
        event: KernelEvent::InsertRecord {
            id: RecordId(id),
            vector: FxpVector::new_zeros(4),
            metadata: None,
            tag: id as u64,
        },
        request_id: None,
        schema_version: 0,
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn two_node_mtls_cluster_elects_and_replicates() {
    let ca = make_ca("valori test cluster CA");
    let n1 = spawn_tls_node(1, tls_config(&ca, &make_leaf(&ca))).await;
    let n2 = spawn_tls_node(2, tls_config(&ca, &make_leaf(&ca))).await;

    let members: BTreeMap<NodeId, ValoriNode> =
        [(1, n1.node.clone()), (2, n2.node.clone())].into_iter().collect();
    n1.raft.initialize(members).await.unwrap();
    n1.raft
        .wait(Some(Duration::from_secs(10)))
        .metrics(|m| m.current_leader.is_some(), "leader over mTLS")
        .await
        .unwrap();

    let leader_id = n1.raft.metrics().borrow().current_leader.unwrap();
    let leader = if leader_id == 1 { &n1 } else { &n2 };

    let mut last_index = 0;
    for i in 0..5u32 {
        last_index = leader.raft.client_write(insert(i)).await.unwrap().data.log_index;
    }

    for n in [&n1, &n2] {
        n.raft
            .wait(Some(Duration::from_secs(10)))
            .applied_index_at_least(Some(last_index), "caught up over mTLS")
            .await
            .unwrap();
    }
    assert_eq!(
        n1.sm.state_hash().await,
        n2.sm.state_hash().await,
        "the SMR invariant holds over the encrypted channel"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn peer_from_a_different_ca_is_refused_at_the_handshake() {
    let cluster_ca = make_ca("real cluster CA");
    let rogue_ca = make_ca("rogue CA");

    // A legitimate node, serving with mutual TLS under the cluster CA.
    let n1 = spawn_tls_node(1, tls_config(&cluster_ca, &make_leaf(&cluster_ca))).await;
    n1.raft
        .initialize(
            [(1, n1.node.clone())].into_iter().collect::<BTreeMap<NodeId, ValoriNode>>(),
        )
        .await
        .unwrap();
    n1.raft
        .wait(Some(Duration::from_secs(10)))
        .metrics(|m| m.current_leader == Some(1), "self-elected")
        .await
        .unwrap();

    // A "node" with a perfectly valid certificate — from the WRONG CA.
    // Its add as learner must fail: replication RPCs die at the handshake.
    let rogue = spawn_tls_node(2, tls_config(&rogue_ca, &make_leaf(&rogue_ca))).await;

    let result = tokio::time::timeout(
        Duration::from_secs(5),
        n1.raft.add_learner(2, rogue.node.clone(), true),
    )
    .await;

    let joined = match result {
        Err(_elapsed) => false,                  // blocked forever at the handshake — refused
        Ok(Err(_)) => false,                     // surfaced an error — refused
        Ok(Ok(_)) => {
            // add_learner returned — but did the rogue actually receive state?
            tokio::time::sleep(Duration::from_millis(500)).await;
            rogue.sm.with_state(|s| s.record_count()).await > 0
                || rogue.raft.metrics().borrow().last_applied.is_some()
        }
    };
    assert!(
        !joined,
        "a peer with a certificate from a different CA must never receive Raft state"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn plaintext_client_cannot_reach_a_tls_server() {
    let ca = make_ca("cluster CA");
    let n1 = spawn_tls_node(1, tls_config(&ca, &make_leaf(&ca))).await;
    n1.raft
        .initialize(
            [(1, n1.node.clone())].into_iter().collect::<BTreeMap<NodeId, ValoriNode>>(),
        )
        .await
        .unwrap();

    // A plain-HTTP gRPC client pointed at the TLS port must fail to carry
    // a single RPC (downgrade is impossible, not just discouraged).
    let endpoint =
        tonic::transport::Endpoint::from_shared(format!("http://{}", n1.node.raft_addr))
            .unwrap()
            .connect_timeout(Duration::from_secs(2));
    let result = tokio::time::timeout(Duration::from_secs(5), async {
        let channel = endpoint.connect().await?;
        let mut client =
            valori_consensus::network::proto::raft_service_client::RaftServiceClient::new(channel);
        client
            .vote(valori_consensus::network::proto::RaftRequest { payload: vec![], shard_id: 0 })
            .await
            .map_err(|e| -> Box<dyn std::error::Error> { Box::new(e) })?;
        Ok::<(), Box<dyn std::error::Error>>(())
    })
    .await;

    let refused = matches!(result, Err(_) | Ok(Err(_)));
    assert!(refused, "plaintext must not reach a TLS-only Raft server");
}
