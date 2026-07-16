// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! `ModelProvider` trait + built-in implementations + [`ProviderRegistry`].

pub mod dummy;
pub mod ollama;
pub mod openai;
pub mod registry;
pub mod voyage;

pub use registry::ProviderRegistry;

use crate::error::ModelResult;

/// One embedding operation — the interface every embedding consumer uses.
///
/// `embed` accepts a batch (`texts`) and returns one `Vec<f32>` per input.
#[async_trait::async_trait]
pub trait ModelProvider: Send + Sync {
    /// Provider kind name (matches `ProviderKind::as_str()`).
    fn kind(&self) -> &'static str;
    /// Human-readable name of the model being served.
    fn model_name(&self) -> &str;
    /// Output dimension.
    fn dim(&self) -> usize;
    /// Embed a batch of texts. Returns one `Vec<f32>` per input.
    async fn embed(&self, texts: &[String]) -> ModelResult<Vec<Vec<f32>>>;
    /// Lightweight liveness check.
    async fn health(&self) -> ModelResult<()>;
}

/// Build a live `ModelProvider` from explicit config params.
///
/// Kept for backward compatibility with `valori-node`'s `EmbedConfig` path.
/// New code should use `ProviderRegistry::build()` directly.
pub fn provider_from_config(
    kind: &str,
    model: &str,
    base_url: Option<&str>,
    api_key: Option<&str>,
    dim: usize,
) -> ModelResult<Box<dyn ModelProvider>> {
    ProviderRegistry::with_defaults().build(kind, model, base_url, api_key, dim)
}
