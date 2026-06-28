// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! gRPC transport between Raft peers — Phase 2.4.
//!
//! Three RPCs (`AppendEntries`, `Vote`, `InstallSnapshot`), each carrying
//! the bincode encoding of the corresponding openraft type. Protobuf is the
//! framing, not the schema — openraft's types are the single source of
//! truth, and replies carry `Result<Resp, RaftError>` so Raft-level errors
//! travel as data while gRPC status codes are reserved for real transport
//! failures.
//!
//! - [`ValoriNetworkFactory`] / [`ValoriNetwork`] — the client side openraft
//!   drives (one lazily-connected channel per peer, reconnect on demand).
//! - [`RaftRpcService`] — the server side: receives an RPC, hands it to the
//!   local `Raft` instance, returns its answer.
//! - [`serve_raft`] — binds the tonic server; returns the bound address
//!   (port 0 supported, for tests).

use openraft::error::{InstallSnapshotError, NetworkError, RPCError, RaftError, RemoteError};
use openraft::network::{RPCOption, RaftNetwork, RaftNetworkFactory};
use openraft::raft::{
    AppendEntriesRequest, AppendEntriesResponse, InstallSnapshotRequest, InstallSnapshotResponse,
    VoteRequest, VoteResponse,
};
use serde::de::DeserializeOwned;
use serde::Serialize;
use tonic::transport::{Channel, Endpoint, Server};

use crate::types::{NodeId, Raft, TypeConfig, ValoriNode};

/// Generated protobuf/tonic code for `proto/raft.proto`.
pub mod proto {
    #![allow(clippy::all)]
    tonic::include_proto!("valori.raft");
}

use proto::raft_service_client::RaftServiceClient;
use proto::raft_service_server::{RaftService, RaftServiceServer};
use proto::{RaftReply, RaftRequest};

fn encode<T: Serialize>(value: &T) -> Result<Vec<u8>, String> {
    bincode::serde::encode_to_vec(value, bincode::config::standard()).map_err(|e| e.to_string())
}

fn decode<T: DeserializeOwned>(bytes: &[u8]) -> Result<T, String> {
    bincode::serde::decode_from_slice(bytes, bincode::config::standard())
        .map(|(v, _)| v)
        .map_err(|e| e.to_string())
}

// ── TLS (Phase 2.10b) ────────────────────────────────────────────────────────

/// PEM material for mutual TLS on the Raft channel.
///
/// Both directions authenticate: the server presents `cert`/`key` and
/// requires a client certificate signed by `ca`; the client presents its
/// own `cert`/`key` and verifies the server against the same `ca`. A peer
/// without a certificate from this cluster's CA is refused at the TLS
/// handshake — it never reaches the Raft layer (the Phase 1.6 contract).
#[derive(Clone)]
pub struct RaftTlsConfig {
    /// Cluster CA certificate (PEM). The single trust root for the cluster.
    pub ca_pem: Vec<u8>,
    /// This node's leaf certificate (PEM), signed by the cluster CA.
    pub cert_pem: Vec<u8>,
    /// This node's private key (PEM).
    pub key_pem: Vec<u8>,
    /// DNS name peers' certificates are issued for (SNI + verification).
    /// One shared name for the whole cluster keeps cert issuance simple;
    /// identity is the CA signature, not the hostname.
    pub domain: String,
}

impl std::fmt::Debug for RaftTlsConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Never print key material — sizes and domain are enough to debug.
        f.debug_struct("RaftTlsConfig")
            .field("ca_pem_len", &self.ca_pem.len())
            .field("cert_pem_len", &self.cert_pem.len())
            .field("key_pem", &"<redacted>")
            .field("domain", &self.domain)
            .finish()
    }
}

// ── Client side ───────────────────────────────────────────────────────────────

/// Builds one [`ValoriNetwork`] per replication target. openraft calls
/// `new_client` with the target's `ValoriNode` from the membership config —
/// the address book is the membership itself, no side table to drift.
/// With a [`RaftTlsConfig`], every outbound channel is mutually
/// authenticated TLS.
#[derive(Default, Clone)]
pub struct ValoriNetworkFactory {
    pub tls: Option<RaftTlsConfig>,
}

impl ValoriNetworkFactory {
    pub fn with_tls(tls: RaftTlsConfig) -> Self {
        Self { tls: Some(tls) }
    }
}

impl RaftNetworkFactory<TypeConfig> for ValoriNetworkFactory {
    type Network = ValoriNetwork;

    async fn new_client(&mut self, target: NodeId, node: &ValoriNode) -> Self::Network {
        ValoriNetwork {
            target,
            raft_addr: node.raft_addr.clone(),
            tls: self.tls.clone(),
            client: None,
        }
    }
}

/// gRPC client for one peer. The channel is created lazily and dropped on
/// any transport error, so the next RPC reconnects — openraft's replication
/// loop supplies the retry cadence.
pub struct ValoriNetwork {
    target: NodeId,
    raft_addr: String,
    tls: Option<RaftTlsConfig>,
    client: Option<RaftServiceClient<Channel>>,
}

impl ValoriNetwork {
    async fn client<E: std::error::Error>(
        &mut self,
    ) -> Result<&mut RaftServiceClient<Channel>, RPCError<NodeId, ValoriNode, E>> {
        if self.client.is_none() {
            let scheme = if self.tls.is_some() { "https" } else { "http" };
            let mut endpoint = Endpoint::from_shared(format!("{scheme}://{}", self.raft_addr))
                .map_err(|e| RPCError::Network(NetworkError::new(&e)))?;

            if let Some(tls) = &self.tls {
                let client_tls = tonic::transport::ClientTlsConfig::new()
                    .ca_certificate(tonic::transport::Certificate::from_pem(&tls.ca_pem))
                    .identity(tonic::transport::Identity::from_pem(
                        &tls.cert_pem,
                        &tls.key_pem,
                    ))
                    .domain_name(&tls.domain);
                endpoint = endpoint
                    .tls_config(client_tls)
                    .map_err(|e| RPCError::Network(NetworkError::new(&e)))?;
            }

            let channel = endpoint
                .connect()
                .await
                .map_err(|e| RPCError::Network(NetworkError::new(&e)))?;
            self.client = Some(RaftServiceClient::new(channel));
        }
        Ok(self.client.as_mut().unwrap())
    }

    fn encode_req<Req: Serialize, E: std::error::Error>(
        req: &Req,
    ) -> Result<RaftRequest, RPCError<NodeId, ValoriNode, E>> {
        let payload =
            encode(req).map_err(|e| RPCError::Network(NetworkError::new(&StrError(e))))?;
        Ok(RaftRequest { payload })
    }

    fn decode_reply<Resp, E>(
        &self,
        reply: RaftReply,
    ) -> Result<Resp, RPCError<NodeId, ValoriNode, E>>
    where
        Resp: DeserializeOwned,
        E: std::error::Error + DeserializeOwned,
    {
        let result: Result<Resp, E> = decode(&reply.payload)
            .map_err(|e| RPCError::Network(NetworkError::new(&StrError(e))))?;
        result.map_err(|raft_err| RPCError::RemoteError(RemoteError::new(self.target, raft_err)))
    }

    fn transport_err<E: std::error::Error>(
        &mut self,
        status: tonic::Status,
    ) -> RPCError<NodeId, ValoriNode, E> {
        // Drop the channel: reconnect on the next attempt.
        self.client = None;
        RPCError::Network(NetworkError::new(&StrError(status.to_string())))
    }
}

#[derive(Debug)]
struct StrError(String);

impl std::fmt::Display for StrError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for StrError {}

impl RaftNetwork<TypeConfig> for ValoriNetwork {
    async fn append_entries(
        &mut self,
        rpc: AppendEntriesRequest<TypeConfig>,
        _option: RPCOption,
    ) -> Result<
        AppendEntriesResponse<NodeId>,
        RPCError<NodeId, ValoriNode, RaftError<NodeId>>,
    > {
        let req = Self::encode_req(&rpc)?;
        let result = self.client().await?.append_entries(req).await;
        match result {
            Ok(reply) => self.decode_reply(reply.into_inner()),
            Err(status) => Err(self.transport_err(status)),
        }
    }

    async fn vote(
        &mut self,
        rpc: VoteRequest<NodeId>,
        _option: RPCOption,
    ) -> Result<VoteResponse<NodeId>, RPCError<NodeId, ValoriNode, RaftError<NodeId>>> {
        let req = Self::encode_req(&rpc)?;
        let result = self.client().await?.vote(req).await;
        match result {
            Ok(reply) => self.decode_reply(reply.into_inner()),
            Err(status) => Err(self.transport_err(status)),
        }
    }

    async fn install_snapshot(
        &mut self,
        rpc: InstallSnapshotRequest<TypeConfig>,
        _option: RPCOption,
    ) -> Result<
        InstallSnapshotResponse<NodeId>,
        RPCError<NodeId, ValoriNode, RaftError<NodeId, InstallSnapshotError>>,
    > {
        let req = Self::encode_req(&rpc)?;
        let result = self.client().await?.install_snapshot(req).await;
        match result {
            Ok(reply) => self.decode_reply(reply.into_inner()),
            Err(status) => Err(self.transport_err(status)),
        }
    }
}

// ── Server side ───────────────────────────────────────────────────────────────

/// The receiving end: unwraps each RPC and hands it to the local `Raft`.
pub struct RaftRpcService {
    raft: Raft,
}

impl RaftRpcService {
    pub fn new(raft: Raft) -> Self {
        Self { raft }
    }
}

fn reply<T: Serialize>(result: &T) -> Result<tonic::Response<RaftReply>, tonic::Status> {
    let payload = encode(result)
        .map_err(|e| tonic::Status::internal(format!("reply encode failed: {e}")))?;
    Ok(tonic::Response::new(RaftReply { payload }))
}

fn bad_request(e: String) -> tonic::Status {
    tonic::Status::invalid_argument(format!("request decode failed: {e}"))
}

#[tonic::async_trait]
impl RaftService for RaftRpcService {
    async fn append_entries(
        &self,
        request: tonic::Request<RaftRequest>,
    ) -> Result<tonic::Response<RaftReply>, tonic::Status> {
        let rpc: AppendEntriesRequest<TypeConfig> =
            decode(&request.into_inner().payload).map_err(bad_request)?;
        let result = self.raft.append_entries(rpc).await;
        reply(&result)
    }

    async fn vote(
        &self,
        request: tonic::Request<RaftRequest>,
    ) -> Result<tonic::Response<RaftReply>, tonic::Status> {
        let rpc: VoteRequest<NodeId> =
            decode(&request.into_inner().payload).map_err(bad_request)?;
        let result = self.raft.vote(rpc).await;
        reply(&result)
    }

    async fn install_snapshot(
        &self,
        request: tonic::Request<RaftRequest>,
    ) -> Result<tonic::Response<RaftReply>, tonic::Status> {
        let rpc: InstallSnapshotRequest<TypeConfig> =
            decode(&request.into_inner().payload).map_err(bad_request)?;
        let result = self.raft.install_snapshot(rpc).await;
        reply(&result)
    }
}

/// Bind the Raft gRPC server on `addr` and serve until the task is dropped.
/// Returns the actually-bound address (so `…:0` works in tests) and the
/// server task handle.
///
/// H-2: This path uses **no authentication**. Any host that can reach the
/// Raft port can inject AppendEntries, Vote, or InstallSnapshot RPCs.
/// Use [`serve_raft_tls`] with mTLS in any non-loopback environment.
pub async fn serve_raft(
    raft: Raft,
    addr: &str,
) -> Result<(std::net::SocketAddr, tokio::task::JoinHandle<()>), std::io::Error> {
    tracing::warn!(
        addr,
        "Raft gRPC starting WITHOUT TLS — any host on the network can inject cluster state. \
         Set VALORI_TLS_CA/CERT/KEY to enable mTLS."
    );
    serve_raft_inner(raft, addr, None).await
}

/// [`serve_raft`] with mutual TLS: the server presents its identity and
/// REQUIRES a client certificate signed by the cluster CA. A peer without
/// one is refused at the handshake — it never reaches the Raft layer.
pub async fn serve_raft_tls(
    raft: Raft,
    addr: &str,
    tls: RaftTlsConfig,
) -> Result<(std::net::SocketAddr, tokio::task::JoinHandle<()>), std::io::Error> {
    serve_raft_inner(raft, addr, Some(tls)).await
}

async fn serve_raft_inner(
    raft: Raft,
    addr: &str,
    tls: Option<RaftTlsConfig>,
) -> Result<(std::net::SocketAddr, tokio::task::JoinHandle<()>), std::io::Error> {
    let listener = tokio::net::TcpListener::bind(addr).await?;
    let bound = listener.local_addr()?;

    let service = RaftServiceServer::new(RaftRpcService::new(raft));
    let incoming = tokio_stream::wrappers::TcpListenerStream::new(listener);

    let mut builder = Server::builder();
    if let Some(tls) = tls {
        let server_tls = tonic::transport::ServerTlsConfig::new()
            .identity(tonic::transport::Identity::from_pem(
                &tls.cert_pem,
                &tls.key_pem,
            ))
            // client_ca_root makes this MUTUAL: peers must present a cert
            // signed by the cluster CA, not merely speak TLS.
            .client_ca_root(tonic::transport::Certificate::from_pem(&tls.ca_pem));
        builder = builder
            .tls_config(server_tls)
            .map_err(|e| std::io::Error::other(format!("server tls config: {e}")))?;
    }

    let handle = tokio::spawn(async move {
        if let Err(e) = builder
            .add_service(service)
            .serve_with_incoming(incoming)
            .await
        {
            tracing::error!("raft rpc server exited with error: {e}");
        }
    });

    Ok((bound, handle))
}
