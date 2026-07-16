// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! On-node HTTP embedding client.
//!
//! Supports three providers:
//! - **ollama** — `POST /api/embed` (≥0.1.36) or `/api/embeddings` (older).
//!   Texts are sent one-at-a-time to avoid context-window overflow.
//! - **openai** — `POST /v1/embeddings` with batched input. Requires an API key.
//! - **custom** — OpenAI-compatible shape at any base URL.

use serde_json::Value;

/// Credentials and routing for a single embedding provider.
///
/// Constructed by the node from `NodeConfig` env vars. Not constructed here —
/// `valori-ingest` has no dependency on `valori-node`'s config types.
#[derive(Clone, Debug)]
pub struct EmbedConfig {
    pub provider: String,
    pub model: String,
    pub url: String,
    pub api_key: Option<String>,
}

/// Returned when an embedding call fails.
#[derive(Debug)]
pub struct EmbedError(pub String);

impl std::fmt::Display for EmbedError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "embed error: {}", self.0)
    }
}

impl std::error::Error for EmbedError {}

/// Embed a batch of text strings. Returns one `Vec<f32>` per input text.
///
/// Ollama is called one text at a time (concatenation blows context windows).
/// OpenAI and custom providers receive all texts in one request.
pub async fn embed_batch(
    texts: &[String],
    cfg: &EmbedConfig,
    http: &reqwest::Client,
) -> Result<Vec<Vec<f32>>, EmbedError> {
    match cfg.provider.as_str() {
        "ollama" => embed_ollama(texts, cfg, http).await,
        "openai" | "custom" => embed_openai_compat(texts, cfg, http).await,
        p => Err(EmbedError(format!(
            "unknown embed provider '{p}'; use ollama, openai, or custom"
        ))),
    }
}

// ── Ollama ────────────────────────────────────────────────────────────────────

async fn embed_ollama(
    texts: &[String],
    cfg: &EmbedConfig,
    http: &reqwest::Client,
) -> Result<Vec<Vec<f32>>, EmbedError> {
    let mut results: Vec<Vec<f32>> = Vec::with_capacity(texts.len());

    for text in texts {
        // Truncate to ~6000 chars to stay inside any model's context window.
        let safe = if text.len() > 6000 { &text[..6000] } else { text.as_str() };

        // Try /api/embed first (Ollama ≥0.1.36).
        let body = serde_json::json!({ "model": cfg.model, "input": safe });
        let res = http
            .post(format!("{}/api/embed", cfg.url))
            .json(&body)
            .send()
            .await
            .map_err(|e| EmbedError(format!("ollama /api/embed: {e}")))?;

        if res.status() == 404 {
            // Fallback to /api/embeddings (Ollama <0.1.36).
            let body2 = serde_json::json!({ "model": cfg.model, "prompt": safe });
            let res2 = http
                .post(format!("{}/api/embeddings", cfg.url))
                .json(&body2)
                .send()
                .await
                .map_err(|e| EmbedError(format!("ollama /api/embeddings: {e}")))?;
            if !res2.status().is_success() {
                return Err(EmbedError(format!(
                    "ollama /api/embeddings HTTP {}",
                    res2.status()
                )));
            }
            let v: Value = res2.json().await.map_err(|e| EmbedError(e.to_string()))?;
            let vec = parse_f32_array(&v["embedding"])
                .ok_or_else(|| EmbedError("ollama: missing 'embedding' field".into()))?;
            results.push(vec);
        } else {
            if !res.status().is_success() {
                let status = res.status();
                let body_txt = res.text().await.unwrap_or_default();
                return Err(EmbedError(format!(
                    "ollama /api/embed HTTP {status}: {body_txt}"
                )));
            }
            let v: Value = res.json().await.map_err(|e| EmbedError(e.to_string()))?;
            let arr = v["embeddings"]
                .as_array()
                .and_then(|a| a.first())
                .ok_or_else(|| EmbedError("ollama: empty 'embeddings' array".into()))?;
            let vec = parse_f32_array(arr)
                .ok_or_else(|| EmbedError("ollama: embeddings[0] is not a float array".into()))?;
            results.push(vec);
        }
    }
    Ok(results)
}

// ── OpenAI-compatible ─────────────────────────────────────────────────────────

async fn embed_openai_compat(
    texts: &[String],
    cfg: &EmbedConfig,
    http: &reqwest::Client,
) -> Result<Vec<Vec<f32>>, EmbedError> {
    // For custom providers: if the URL already contains a path, use as-is;
    // otherwise append the canonical /v1/embeddings path.
    let endpoint = if cfg.url.contains("/v1/") || cfg.url.ends_with("/embed") {
        cfg.url.clone()
    } else {
        format!("{}/v1/embeddings", cfg.url)
    };

    let body = serde_json::json!({ "model": cfg.model, "input": texts });

    let mut req = http.post(&endpoint).json(&body);
    if let Some(ref key) = cfg.api_key {
        req = req.bearer_auth(key);
    }

    let res = req
        .send()
        .await
        .map_err(|e| EmbedError(format!("embed POST {endpoint}: {e}")))?;

    if !res.status().is_success() {
        let status = res.status();
        let txt = res.text().await.unwrap_or_default();
        return Err(EmbedError(format!("embed {status}: {txt}")));
    }

    let v: Value = res.json().await.map_err(|e| EmbedError(e.to_string()))?;

    // Standard OpenAI: { data: [ { embedding: [f32] }, … ] }
    if let Some(data) = v["data"].as_array() {
        return data
            .iter()
            .map(|item| {
                parse_f32_array(&item["embedding"])
                    .ok_or_else(|| EmbedError("openai: missing embedding in data item".into()))
            })
            .collect();
    }

    // Fallback: { embeddings: [[f32]] }
    if let Some(arr) = v["embeddings"].as_array() {
        return arr
            .iter()
            .map(|item| {
                parse_f32_array(item).ok_or_else(|| {
                    EmbedError("custom: embeddings item is not a float array".into())
                })
            })
            .collect();
    }

    Err(EmbedError(format!(
        "embed: unexpected response shape: {}",
        v
    )))
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn parse_f32_array(v: &Value) -> Option<Vec<f32>> {
    v.as_array().map(|a| {
        a.iter()
            .filter_map(|x| x.as_f64().map(|f| f as f32))
            .collect()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_provider_errors() {
        let cfg = EmbedConfig {
            provider: "unknown".into(),
            model: "model".into(),
            url: "http://localhost".into(),
            api_key: None,
        };
        let client = reqwest::Client::new();
        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt.block_on(embed_batch(&["hello".into()], &cfg, &client));
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("unknown embed provider"));
    }

    #[test]
    fn embed_error_display() {
        let e = EmbedError("something went wrong".into());
        assert_eq!(e.to_string(), "embed error: something went wrong");
    }
}
