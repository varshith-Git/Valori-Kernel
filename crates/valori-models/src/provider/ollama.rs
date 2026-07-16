// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! `OllamaProvider` — embeds via a locally-running Ollama instance.
//!
//! Handles both the newer `/api/embed` (batch) and the legacy
//! `/api/embeddings` (single-text) endpoints.

use super::ModelProvider;
use crate::error::{ModelError, ModelResult};

pub struct OllamaProvider {
    model: String,
    base_url: String,
    dim: usize,
    client: reqwest::Client,
}

impl OllamaProvider {
    pub fn new(model: impl Into<String>, base_url: impl Into<String>, dim: usize) -> Self {
        Self {
            model: model.into(),
            base_url: base_url.into().trim_end_matches('/').to_string(),
            dim,
            client: reqwest::Client::new(),
        }
    }
}

#[async_trait::async_trait]
impl ModelProvider for OllamaProvider {
    fn kind(&self) -> &'static str {
        "ollama"
    }
    fn model_name(&self) -> &str {
        &self.model
    }
    fn dim(&self) -> usize {
        self.dim
    }

    async fn embed(&self, texts: &[String]) -> ModelResult<Vec<Vec<f32>>> {
        // Try the batch endpoint first (/api/embed, Ollama ≥0.1.36).
        let resp = self
            .client
            .post(format!("{}/api/embed", self.base_url))
            .json(&serde_json::json!({ "model": self.model, "input": texts }))
            .send()
            .await;

        match resp {
            Ok(r) if r.status().is_success() => {
                let body: serde_json::Value = r
                    .json()
                    .await
                    .map_err(|e| ModelError::Provider(format!("ollama /api/embed parse: {e}")))?;
                let embeddings = body["embeddings"]
                    .as_array()
                    .ok_or_else(|| ModelError::Provider("ollama: missing 'embeddings'".into()))?;
                embeddings
                    .iter()
                    .map(|v| {
                        v.as_array()
                            .ok_or_else(|| {
                                ModelError::Provider("ollama: non-array embedding".into())
                            })?
                            .iter()
                            .map(|x| {
                                x.as_f64().map(|f| f as f32).ok_or_else(|| {
                                    ModelError::Provider("non-float in embedding".into())
                                })
                            })
                            .collect::<ModelResult<Vec<f32>>>()
                    })
                    .collect()
            }
            _ => {
                // Legacy endpoint: one request per text.
                let mut out = Vec::with_capacity(texts.len());
                for text in texts {
                    let r = self
                        .client
                        .post(format!("{}/api/embeddings", self.base_url))
                        .json(&serde_json::json!({ "model": self.model, "prompt": text }))
                        .send()
                        .await
                        .map_err(|e| ModelError::Provider(format!("ollama legacy: {e}")))?;
                    if !r.status().is_success() {
                        return Err(ModelError::Provider(format!(
                            "ollama legacy HTTP {}",
                            r.status()
                        )));
                    }
                    let body: serde_json::Value = r
                        .json()
                        .await
                        .map_err(|e| ModelError::Provider(e.to_string()))?;
                    let vec: Vec<f32> = body["embedding"]
                        .as_array()
                        .ok_or_else(|| ModelError::Provider("ollama: missing 'embedding'".into()))?
                        .iter()
                        .map(|x| {
                            x.as_f64()
                                .map(|f| f as f32)
                                .ok_or_else(|| ModelError::Provider("non-float".into()))
                        })
                        .collect::<ModelResult<_>>()?;
                    out.push(vec);
                }
                Ok(out)
            }
        }
    }

    async fn health(&self) -> ModelResult<()> {
        let r = self
            .client
            .get(format!("{}/api/tags", self.base_url))
            .timeout(std::time::Duration::from_secs(3))
            .send()
            .await
            .map_err(|e| ModelError::Provider(format!("ollama health: {e}")))?;
        if r.status().is_success() {
            Ok(())
        } else {
            Err(ModelError::Provider(format!(
                "ollama health HTTP {}",
                r.status()
            )))
        }
    }
}
