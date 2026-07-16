// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! The node-facing backend: a thin async client over the primitive Valori HTTP
//! endpoints the MCP tools compose. Defining it as a trait lets the tool layer
//! (and its tests) run against either the real [`HttpBackend`] or an in-memory
//! fake, so the receipt-assembly logic is provable without a live node.

use anyhow::{Context, Result};
use async_trait::async_trait;
use serde_json::{json, Value};

/// The set of primitive node operations the MCP tools are built from. These map
/// 1:1 to existing Valori endpoints — the MCP server adds no new server logic,
/// it only composes (e.g. recall = search + proof → receipt).
#[async_trait]
pub trait NodeClient: Send + Sync {
    /// `POST /v1/memory/upsert_vector`
    async fn memory_upsert(
        &self,
        vector: Vec<f32>,
        collection: Option<String>,
        metadata: Option<Value>,
    ) -> Result<Value>;

    /// `POST /v1/memory/search_vector`
    #[allow(clippy::too_many_arguments)]
    async fn memory_search(
        &self,
        query_vector: Vec<f32>,
        k: usize,
        collection: Option<String>,
        decay_half_life_secs: Option<u64>,
        metadata_filter: Option<Value>,
        rerank: bool,
        query_text: Option<String>,
    ) -> Result<Value>;

    /// `GET /v1/proof/state` → 64-hex state hash.
    async fn proof_state(&self) -> Result<String>;

    /// `GET /v1/proof/event-log` → `(event_log_hash, committed_height)` when an
    /// event log is enabled on the node; `None` otherwise (in-memory node).
    async fn proof_event_log(&self) -> Result<Option<(String, u64)>>;

    /// `GET /graph/subgraph?root=&depth=` — provenance neighbourhood.
    async fn subgraph(&self, root: u32, depth: u32) -> Result<Value>;

    /// `POST /v1/graphrag` — KNN + subgraph expansion in one snapshot.
    async fn graphrag(
        &self,
        query_vector: Vec<f32>,
        k: usize,
        depth: u32,
        collection: Option<String>,
    ) -> Result<Value>;

    /// `GET /v1/timeline` — committed event history.
    async fn timeline(&self, from: Option<String>, to: Option<String>) -> Result<Value>;

    /// `DELETE /v1/crypto/shred/:key_id` — certified erasure.
    async fn crypto_shred(&self, key_id: String) -> Result<Value>;

    /// `POST /v1/snapshot/save` — deterministic snapshot = a fork point.
    async fn snapshot_save(&self) -> Result<Value>;
}

/// HTTP implementation backed by `reqwest`, talking to a running Valori node.
pub struct HttpBackend {
    base_url: String,
    auth_token: Option<String>,
    http: reqwest::Client,
}

impl HttpBackend {
    pub fn new(base_url: impl Into<String>, auth_token: Option<String>) -> Self {
        let base_url = base_url.into();
        let base_url = base_url.trim_end_matches('/').to_string();
        Self {
            base_url,
            auth_token,
            http: reqwest::Client::new(),
        }
    }

    fn url(&self, path: &str) -> String {
        format!("{}{}", self.base_url, path)
    }

    fn auth(&self, rb: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        match &self.auth_token {
            Some(t) => rb.bearer_auth(t),
            None => rb,
        }
    }

    async fn get_json(&self, path: &str, query: &[(&str, String)]) -> Result<Value> {
        let req = self.auth(self.http.get(self.url(path)).query(query));
        let resp = req.send().await.with_context(|| format!("GET {path}"))?;
        Self::json_or_err(path, resp).await
    }

    async fn post_json(&self, path: &str, body: Value) -> Result<Value> {
        let req = self.auth(self.http.post(self.url(path)).json(&body));
        let resp = req.send().await.with_context(|| format!("POST {path}"))?;
        Self::json_or_err(path, resp).await
    }

    async fn delete_json(&self, path: &str) -> Result<Value> {
        let req = self.auth(self.http.delete(self.url(path)));
        let resp = req.send().await.with_context(|| format!("DELETE {path}"))?;
        Self::json_or_err(path, resp).await
    }

    async fn json_or_err(path: &str, resp: reqwest::Response) -> Result<Value> {
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        if !status.is_success() {
            anyhow::bail!("node returned {status} for {path}: {text}");
        }
        if text.is_empty() {
            return Ok(json!({}));
        }
        serde_json::from_str(&text).with_context(|| format!("decoding {path} response: {text}"))
    }
}

#[async_trait]
impl NodeClient for HttpBackend {
    async fn memory_upsert(
        &self,
        vector: Vec<f32>,
        collection: Option<String>,
        metadata: Option<Value>,
    ) -> Result<Value> {
        let mut body = json!({ "vector": vector });
        if let Some(c) = collection {
            body["collection"] = json!(c);
        }
        if let Some(m) = metadata {
            body["metadata"] = m;
        }
        self.post_json("/v1/memory/upsert_vector", body).await
    }

    #[allow(clippy::too_many_arguments)]
    async fn memory_search(
        &self,
        query_vector: Vec<f32>,
        k: usize,
        collection: Option<String>,
        decay_half_life_secs: Option<u64>,
        metadata_filter: Option<Value>,
        rerank: bool,
        query_text: Option<String>,
    ) -> Result<Value> {
        let mut body = json!({ "query_vector": query_vector, "k": k });
        if let Some(c) = collection {
            body["collection"] = json!(c);
        }
        if let Some(h) = decay_half_life_secs.filter(|&v| v > 0) {
            body["decay_half_life_secs"] = json!(h);
        }
        if let Some(f) = metadata_filter {
            body["metadata_filter"] = f;
        }
        if rerank {
            body["rerank"] = json!(true);
            if let Some(t) = query_text {
                body["query_text"] = json!(t);
            }
        }
        self.post_json("/v1/memory/search_vector", body).await
    }

    async fn proof_state(&self) -> Result<String> {
        let v = self.get_json("/v1/proof/state", &[]).await?;
        v.get("final_state_hash")
            .and_then(|h| h.as_str())
            .map(|s| s.to_string())
            .context("proof/state response missing final_state_hash")
    }

    async fn proof_event_log(&self) -> Result<Option<(String, u64)>> {
        // The node returns 400 (BAD_REQUEST) when no event log is enabled —
        // treat that as "no event-log proof available" rather than a hard error,
        // so recall still works against an in-memory node (with a weaker receipt).
        //
        // Transport errors (connection refused, timeout, DNS) are NOT silently
        // swallowed: if the node was reachable for proof_state but then fails
        // here, the receipt would be silently downgraded. Propagate those as Err.
        let req = self.auth(self.http.get(self.url("/v1/proof/event-log")));
        let resp = req.send().await.context("GET /v1/proof/event-log")?;
        if !resp.status().is_success() {
            // Any 4xx/5xx → node has no event log or it is temporarily
            // unavailable. Return None so the receipt is issued without the
            // event-log binding rather than failing the entire recall.
            return Ok(None);
        }
        let v: Value = resp.json().await.unwrap_or(json!({}));
        let hash = v
            .get("event_log_hash")
            .and_then(|h| h.as_str())
            .map(|s| s.to_string());
        let height = v.get("committed_height").and_then(|h| h.as_u64());
        match (hash, height) {
            (Some(h), Some(n)) => Ok(Some((h, n))),
            _ => Ok(None),
        }
    }

    async fn subgraph(&self, root: u32, depth: u32) -> Result<Value> {
        self.get_json(
            "/graph/subgraph",
            &[("root", root.to_string()), ("depth", depth.to_string())],
        )
        .await
    }

    async fn graphrag(
        &self,
        query_vector: Vec<f32>,
        k: usize,
        depth: u32,
        collection: Option<String>,
    ) -> Result<Value> {
        let mut body = json!({ "query_vector": query_vector, "k": k, "depth": depth });
        if let Some(c) = collection {
            body["collection"] = json!(c);
        }
        self.post_json("/v1/graphrag", body).await
    }

    async fn timeline(&self, from: Option<String>, to: Option<String>) -> Result<Value> {
        let mut q: Vec<(&str, String)> = Vec::new();
        if let Some(f) = from {
            q.push(("from", f));
        }
        if let Some(t) = to {
            q.push(("to", t));
        }
        self.get_json("/v1/timeline", &q).await
    }

    async fn crypto_shred(&self, key_id: String) -> Result<Value> {
        self.delete_json(&format!("/v1/crypto/shred/{key_id}"))
            .await
    }

    async fn snapshot_save(&self) -> Result<Value> {
        self.post_json("/v1/snapshot/save", json!({})).await
    }
}
