// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! [`ProviderRegistry`] — M2.
//!
//! Replaces the `match kind { ... }` dispatch in `provider_from_config`.
//! Each provider registers a factory; callers never need match statements.

use std::collections::HashMap;

use crate::error::{ModelError, ModelResult};
use crate::manifest::ModelManifest;
use crate::provider::{
    dummy::DummyProvider, ollama::OllamaProvider, openai::OpenAIProvider, voyage::VoyageProvider,
    ModelProvider,
};

/// Factory that knows how to construct one kind of `ModelProvider`.
pub trait ProviderFactory: Send + Sync {
    /// Lowercase tag matching `ProviderKind::as_str()`.
    fn kind(&self) -> &'static str;

    /// Build from explicit config params (used when loading from env vars).
    fn build_from_params(
        &self,
        model: &str,
        base_url: Option<&str>,
        api_key: Option<&str>,
        dim: usize,
    ) -> ModelResult<Box<dyn ModelProvider>>;

    /// Build from an installed manifest (default: delegates to `build_from_params`).
    fn build_from_manifest(&self, manifest: &ModelManifest) -> ModelResult<Box<dyn ModelProvider>> {
        self.build_from_params(
            manifest_model_name(manifest),
            None,
            None,
            manifest.dimensions,
        )
    }
}

/// Central registry of provider factories.
///
/// Call [`ProviderRegistry::with_defaults()`] to get all built-in providers
/// pre-registered. Downstream code can call [`register`] to add more.
pub struct ProviderRegistry {
    factories: HashMap<String, Box<dyn ProviderFactory>>,
}

impl ProviderRegistry {
    pub fn new() -> Self {
        Self {
            factories: HashMap::new(),
        }
    }

    /// Return a registry with all built-in providers registered.
    pub fn with_defaults() -> Self {
        let mut r = Self::new();
        r.register(Box::new(OllamaFactory));
        r.register(Box::new(OpenAIFactory));
        r.register(Box::new(VoyageFactory));
        r.register(Box::new(DummyFactory));
        r
    }

    /// Register a provider factory. Last registration for a given kind wins.
    pub fn register(&mut self, factory: Box<dyn ProviderFactory>) {
        self.factories.insert(factory.kind().to_string(), factory);
    }

    /// List all registered provider kind names.
    pub fn provider_kinds(&self) -> Vec<&str> {
        let mut kinds: Vec<&str> = self.factories.keys().map(|s| s.as_str()).collect();
        kinds.sort_unstable();
        kinds
    }

    /// Build a provider from explicit params — used when config is the source.
    pub fn build(
        &self,
        kind: &str,
        model: &str,
        base_url: Option<&str>,
        api_key: Option<&str>,
        dim: usize,
    ) -> ModelResult<Box<dyn ModelProvider>> {
        let factory = self
            .factories
            .get(kind)
            .ok_or_else(|| ModelError::Provider(format!("unknown provider '{kind}'")))?;
        factory.build_from_params(model, base_url, api_key, dim)
    }

    /// Build a provider from an installed manifest.
    pub fn build_from_manifest(
        &self,
        manifest: &ModelManifest,
    ) -> ModelResult<Box<dyn ModelProvider>> {
        let kind = manifest.provider.as_str();
        let factory = self.factories.get(kind).ok_or_else(|| {
            ModelError::Provider(format!("no factory registered for provider '{kind}'"))
        })?;
        factory.build_from_manifest(manifest)
    }

    /// Return the first installed manifest's provider as the default, or the
    /// first registered factory kind when nothing is installed.
    pub fn default_kind(&self) -> Option<&str> {
        self.factories.keys().next().map(|s| s.as_str())
    }
}

impl Default for ProviderRegistry {
    fn default() -> Self {
        Self::with_defaults()
    }
}

// ── Built-in factories ────────────────────────────────────────────────────────

struct OllamaFactory;

impl ProviderFactory for OllamaFactory {
    fn kind(&self) -> &'static str {
        "ollama"
    }

    fn build_from_params(
        &self,
        model: &str,
        base_url: Option<&str>,
        _api_key: Option<&str>,
        dim: usize,
    ) -> ModelResult<Box<dyn ModelProvider>> {
        Ok(Box::new(OllamaProvider::new(
            model,
            base_url.unwrap_or("http://localhost:11434"),
            dim,
        )))
    }
}

struct OpenAIFactory;

impl ProviderFactory for OpenAIFactory {
    fn kind(&self) -> &'static str {
        "openai"
    }

    fn build_from_params(
        &self,
        model: &str,
        base_url: Option<&str>,
        api_key: Option<&str>,
        dim: usize,
    ) -> ModelResult<Box<dyn ModelProvider>> {
        Ok(Box::new(OpenAIProvider::new(
            model,
            base_url.unwrap_or("https://api.openai.com"),
            api_key.unwrap_or(""),
            dim,
        )))
    }
}

// `custom` is an alias for OpenAI-compatible. Register separately so both
// "openai" and "custom" resolve to the same factory logic.
#[allow(dead_code)]
struct CustomFactory;

impl ProviderFactory for CustomFactory {
    fn kind(&self) -> &'static str {
        "custom"
    }

    fn build_from_params(
        &self,
        model: &str,
        base_url: Option<&str>,
        api_key: Option<&str>,
        dim: usize,
    ) -> ModelResult<Box<dyn ModelProvider>> {
        Ok(Box::new(OpenAIProvider::new(
            model,
            base_url.unwrap_or("http://localhost:8080"),
            api_key.unwrap_or(""),
            dim,
        )))
    }
}

struct VoyageFactory;

impl ProviderFactory for VoyageFactory {
    fn kind(&self) -> &'static str {
        "voyage"
    }

    fn build_from_params(
        &self,
        model: &str,
        _base_url: Option<&str>,
        api_key: Option<&str>,
        dim: usize,
    ) -> ModelResult<Box<dyn ModelProvider>> {
        Ok(Box::new(VoyageProvider::new(
            model,
            api_key.unwrap_or(""),
            dim,
        )))
    }
}

struct DummyFactory;

impl ProviderFactory for DummyFactory {
    fn kind(&self) -> &'static str {
        "dummy"
    }

    fn build_from_params(
        &self,
        _model: &str,
        _base_url: Option<&str>,
        _api_key: Option<&str>,
        dim: usize,
    ) -> ModelResult<Box<dyn ModelProvider>> {
        Ok(Box::new(DummyProvider::new(dim)))
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Extract the short model name from a manifest id (`provider/name` → `name`).
fn manifest_model_name(manifest: &ModelManifest) -> &str {
    manifest.id.rsplit('/').next().unwrap_or(&manifest.id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_lists_built_in_providers() {
        let r = ProviderRegistry::with_defaults();
        let kinds = r.provider_kinds();
        assert!(kinds.contains(&"ollama"));
        assert!(kinds.contains(&"openai"));
        assert!(kinds.contains(&"voyage"));
        assert!(kinds.contains(&"dummy"));
    }

    #[test]
    fn unknown_provider_errors() {
        let r = ProviderRegistry::with_defaults();
        let err = r.build("onnx", "model", None, None, 384).err().unwrap();
        assert!(err.to_string().contains("onnx"));
    }

    #[test]
    fn dummy_provider_builds() {
        let r = ProviderRegistry::with_defaults();
        let p = r.build("dummy", "dummy", None, None, 4).unwrap();
        assert_eq!(p.dim(), 4);
    }

    #[test]
    fn custom_factory_can_be_registered() {
        let mut r = ProviderRegistry::new();
        r.register(Box::new(CustomFactory));
        let p = r
            .build(
                "custom",
                "my-model",
                Some("http://localhost:8080"),
                None,
                768,
            )
            .unwrap();
        assert_eq!(p.dim(), 768);
    }
}
