// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! `DummyProvider` — deterministic zero vectors, for tests only.

use super::ModelProvider;
use crate::error::ModelResult;

pub struct DummyProvider {
    dim: usize,
}

impl DummyProvider {
    pub fn new(dim: usize) -> Self {
        Self { dim }
    }
}

#[async_trait::async_trait]
impl ModelProvider for DummyProvider {
    fn kind(&self) -> &'static str {
        "dummy"
    }
    fn model_name(&self) -> &str {
        "dummy"
    }
    fn dim(&self) -> usize {
        self.dim
    }

    async fn embed(&self, texts: &[String]) -> ModelResult<Vec<Vec<f32>>> {
        Ok(texts.iter().map(|_| vec![0.0f32; self.dim]).collect())
    }

    async fn health(&self) -> ModelResult<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn dummy_returns_zero_vectors() {
        let p = DummyProvider::new(4);
        let out = p.embed(&["hello".into(), "world".into()]).await.unwrap();
        assert_eq!(out.len(), 2);
        assert_eq!(out[0], vec![0.0; 4]);
        assert_eq!(out[1], vec![0.0; 4]);
    }
}
