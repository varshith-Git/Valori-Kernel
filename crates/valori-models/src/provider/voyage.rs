// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! `VoyageProvider` — embeds via Voyage AI.

use super::ModelProvider;
use crate::error::{ModelError, ModelResult};

const VOYAGE_BASE: &str = "https://api.voyageai.com/v1";

pub struct VoyageProvider {
    model: String,
    api_key: String,
    dim: usize,
    client: reqwest::Client,
}

impl VoyageProvider {
    pub fn new(model: impl Into<String>, api_key: impl Into<String>, dim: usize) -> Self {
        Self {
            model: model.into(),
            api_key: api_key.into(),
            dim,
            client: reqwest::Client::new(),
        }
    }
}

#[async_trait::async_trait]
impl ModelProvider for VoyageProvider {
    fn kind(&self) -> &'static str {
        "voyage"
    }
    fn model_name(&self) -> &str {
        &self.model
    }
    fn dim(&self) -> usize {
        self.dim
    }

    async fn embed(&self, texts: &[String]) -> ModelResult<Vec<Vec<f32>>> {
        let resp = self
            .client
            .post(format!("{VOYAGE_BASE}/embeddings"))
            .bearer_auth(&self.api_key)
            .json(&serde_json::json!({ "model": self.model, "input": texts }))
            .send()
            .await
            .map_err(|e| ModelError::Provider(format!("voyage request: {e}")))?;

        if !resp.status().is_success() {
            let code = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(ModelError::Provider(format!("voyage HTTP {code}: {body}")));
        }

        let body: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| ModelError::Provider(format!("voyage parse: {e}")))?;
        let data = body["data"]
            .as_array()
            .ok_or_else(|| ModelError::Provider("voyage: missing 'data'".into()))?;
        data.iter()
            .map(|item| {
                item["embedding"]
                    .as_array()
                    .ok_or_else(|| ModelError::Provider("voyage: missing 'embedding'".into()))?
                    .iter()
                    .map(|x| {
                        x.as_f64()
                            .map(|f| f as f32)
                            .ok_or_else(|| ModelError::Provider("non-float".into()))
                    })
                    .collect::<ModelResult<Vec<f32>>>()
            })
            .collect()
    }

    async fn health(&self) -> ModelResult<()> {
        Ok(()) // no lightweight Voyage health endpoint; treat as always available
    }
}
