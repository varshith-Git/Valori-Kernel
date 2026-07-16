// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! [`Embedder`] trait + [`ModelProviderEmbedder`] — the embedding stage.
//!
//! The embedder knows nothing about Ollama, OpenAI, or Voyage.
//! It delegates to whatever [`valori_models::ModelProvider`] it was given.

use crate::document::{Chunk, Embedding, IngestError};
use valori_models::ModelProvider;

/// Turns a batch of [`Chunk`]s into [`Embedding`]s.
///
/// The implementation decides which model, which API, which HTTP client.
/// `IngestPipeline` doesn't know or care.
#[async_trait::async_trait]
pub trait Embedder: Send + Sync {
    async fn embed(&self, chunks: &[Chunk]) -> Result<Vec<Embedding>, IngestError>;
}

/// Delegates embedding to a [`ModelProvider`] from `valori-models`.
///
/// Constructed with any provider the caller chooses — Ollama, OpenAI, Voyage,
/// or the test `DummyProvider`. The embedder stage is oblivious to which.
pub struct ModelProviderEmbedder {
    provider: Box<dyn ModelProvider>,
}

impl ModelProviderEmbedder {
    pub fn new(provider: Box<dyn ModelProvider>) -> Self {
        Self { provider }
    }
}

#[async_trait::async_trait]
impl Embedder for ModelProviderEmbedder {
    async fn embed(&self, chunks: &[Chunk]) -> Result<Vec<Embedding>, IngestError> {
        let texts: Vec<String> = chunks.iter().map(|c| c.text.clone()).collect();
        let vecs = self
            .provider
            .embed(&texts)
            .await
            .map_err(|e| IngestError::Embed(e.to_string()))?;
        let model_id = format!("{}/{}", self.provider.kind(), self.provider.model_name());
        Ok(chunks
            .iter()
            .zip(vecs)
            .map(|(chunk, values)| Embedding {
                chunk_id: chunk.id.clone(),
                model_id: model_id.clone(),
                dimensions: values.len(),
                values,
            })
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use valori_models::ModelError;

    struct ConstantProvider(usize);

    #[async_trait::async_trait]
    impl ModelProvider for ConstantProvider {
        fn kind(&self) -> &'static str { "test" }
        fn model_name(&self) -> &str { "constant" }
        fn dim(&self) -> usize { self.0 }
        async fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, ModelError> {
            Ok(texts.iter().map(|_| vec![1.0f32; self.0]).collect())
        }
        async fn health(&self) -> Result<(), ModelError> { Ok(()) }
    }

    #[tokio::test]
    async fn produces_one_embedding_per_chunk() {
        let embedder = ModelProviderEmbedder::new(Box::new(ConstantProvider(4)));
        let chunks = vec![
            Chunk::new(0, "", "hello"),
            Chunk::new(1, "", "world"),
        ];
        let out = embedder.embed(&chunks).await.unwrap();
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].values, vec![1.0; 4]);
        assert_eq!(out[0].chunk_id, chunks[0].id);
        assert_eq!(out[0].dimensions, 4);
        assert!(out[0].model_id.contains("constant"));
    }
}
