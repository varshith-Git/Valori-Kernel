// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! `OpenAIProvider` — embeds via OpenAI (or any OpenAI-compatible API).

use super::ModelProvider;
use crate::error::{ModelError, ModelResult};

pub struct OpenAIProvider {
    model: String,
    base_url: String,
    api_key: String,
    dim: usize,
    client: reqwest::Client,
}

impl OpenAIProvider {
    pub fn new(
        model: impl Into<String>,
        base_url: impl Into<String>,
        api_key: impl Into<String>,
        dim: usize,
    ) -> Self {
        Self {
            model: model.into(),
            base_url: base_url.into().trim_end_matches('/').to_string(),
            api_key: api_key.into(),
            dim,
            client: reqwest::Client::new(),
        }
    }
}

#[async_trait::async_trait]
impl ModelProvider for OpenAIProvider {
    fn kind(&self) -> &'static str {
        "openai"
    }
    fn model_name(&self) -> &str {
        &self.model
    }
    fn dim(&self) -> usize {
        self.dim
    }

    async fn embed(&self, texts: &[String]) -> ModelResult<Vec<Vec<f32>>> {
        let url = if self.base_url.contains("/v1") {
            format!("{}/embeddings", self.base_url)
        } else {
            format!("{}/v1/embeddings", self.base_url)
        };

        let resp = self
            .client
            .post(&url)
            .bearer_auth(&self.api_key)
            .json(&serde_json::json!({ "model": self.model, "input": texts }))
            .send()
            .await
            .map_err(|e| ModelError::Provider(format!("openai request: {e}")))?;

        if !resp.status().is_success() {
            let code = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(ModelError::Provider(format!("openai HTTP {code}: {body}")));
        }

        let body: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| ModelError::Provider(format!("openai parse: {e}")))?;
        let data = body["data"]
            .as_array()
            .ok_or_else(|| ModelError::Provider("openai: missing 'data'".into()))?;
        data.iter()
            .map(|item| {
                item["embedding"]
                    .as_array()
                    .ok_or_else(|| ModelError::Provider("openai: missing 'embedding'".into()))?
                    .iter()
                    .map(|x| {
                        x.as_f64()
                            .map(|f| f as f32)
                            .ok_or_else(|| ModelError::Provider("non-float in embedding".into()))
                    })
                    .collect::<ModelResult<Vec<f32>>>()
            })
            .collect()
    }

    async fn health(&self) -> ModelResult<()> {
        // A lightweight models list; if it 401s the key is wrong but the API is up.
        let url = if self.base_url.contains("/v1") {
            format!("{}/models", self.base_url)
        } else {
            format!("{}/v1/models", self.base_url)
        };
        let r = self
            .client
            .get(&url)
            .bearer_auth(&self.api_key)
            .timeout(std::time::Duration::from_secs(5))
            .send()
            .await
            .map_err(|e| ModelError::Provider(format!("openai health: {e}")))?;
        match r.status().as_u16() {
            200 | 401 => Ok(()), // 401 = wrong key, but service is up
            code => Err(ModelError::Provider(format!("openai health HTTP {code}"))),
        }
    }
}
