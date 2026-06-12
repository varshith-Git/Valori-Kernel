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

// ── Client side ───────────────────────────────────────────────────────────────

/// Builds one [`ValoriNetwork`] per replication target. openraft calls
/// `new_client` with the target's `ValoriNode` from the membership config —
/// the address book is the membership itself, no side table to drift.
#[derive(Default, Clone)]
pub struct ValoriNetworkFactory;

impl RaftNetworkFactory<TypeConfig> for ValoriNetworkFactory {
    type Network = ValoriNetwork;

    async fn new_client(&mut self, target: NodeId, node: &ValoriNode) -> Self::Network {
        ValoriNetwork {
            target,
            raft_addr: node.raft_addr.clone(),
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
    client: Option<RaftServiceClient<Channel>>,
}

impl ValoriNetwork {
    async fn client<E: std::error::Error>(
        &mut self,
    ) -> Result<&mut RaftServiceClient<Channel>, RPCError<NodeId, ValoriNode, E>> {
        if self.client.is_none() {
            let endpoint = Endpoint::from_shared(format!("http://{}", self.raft_addr))
                .map_err(|e| RPCError::Network(NetworkError::new(&e)))?;
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
pub async fn serve_raft(
    raft: Raft,
    addr: &str,
) -> Result<(std::net::SocketAddr, tokio::task::JoinHandle<()>), std::io::Error> {
    let listener = tokio::net::TcpListener::bind(addr).await?;
    let bound = listener.local_addr()?;

    let service = RaftServiceServer::new(RaftRpcService::new(raft));
    let incoming = tokio_stream::wrappers::TcpListenerStream::new(listener);

    let handle = tokio::spawn(async move {
        if let Err(e) = Server::builder()
            .add_service(service)
            .serve_with_incoming(incoming)
            .await
        {
            tracing::error!("raft rpc server exited with error: {e}");
        }
    });

    Ok((bound, handle))
}
