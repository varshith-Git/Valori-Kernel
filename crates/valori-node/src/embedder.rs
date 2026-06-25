/// On-node HTTP embedding client.
///
/// Reads provider config from NodeConfig (populated from env vars).
/// Supports Ollama, OpenAI, and a generic custom endpoint.
///
/// Providers
/// ---------
/// ollama  — POST /api/embed  { model, input: str }  → { embeddings: [[f32]] }
///           Falls back to /api/embeddings { model, prompt } for older Ollama.
///           Default URL: http://localhost:11434
///
/// openai  — POST /v1/embeddings  { model, input: [str] }  → { data: [{embedding}] }
///           Default URL: https://api.openai.com
///           Requires VALORI_EMBED_API_KEY.
///
/// custom  — POST <VALORI_EMBED_URL>/v1/embeddings  (OpenAI-compatible shape)
///           or POST <VALORI_EMBED_URL> if it ends with a path segment.

use serde_json::Value;

#[derive(Clone, Debug)]
pub struct EmbedConfig {
    pub provider: String,
    pub model:    String,
    pub url:      String,
    pub api_key:  Option<String>,
}

impl EmbedConfig {
    pub fn from_node_config(cfg: &crate::config::NodeConfig) -> Option<Self> {
        let provider = cfg.embed_provider.clone()?;
        let model = cfg.embed_model.clone().unwrap_or_else(|| match provider.as_str() {
            "openai" => "text-embedding-3-small".into(),
            _        => "nomic-embed-text".into(),
        });
        let url = cfg.embed_url.clone().unwrap_or_else(|| match provider.as_str() {
            "openai" => "https://api.openai.com".into(),
            _        => "http://localhost:11434".into(),
        });
        Some(EmbedConfig {
            provider,
            model,
            url: url.trim_end_matches('/').to_string(),
            api_key: cfg.embed_api_key.clone(),
        })
    }
}

#[derive(Debug)]
pub struct EmbedError(pub String);

impl std::fmt::Display for EmbedError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "embed error: {}", self.0)
    }
}

impl std::error::Error for EmbedError {}

/// Embed a batch of text strings. Returns one `Vec<f32>` per input text.
/// Texts are sent one-at-a-time to Ollama (which concatenates batch inputs)
/// but batched for OpenAI/custom to minimise round-trips.
pub async fn embed_batch(
    texts: &[String],
    cfg: &EmbedConfig,
    http: &reqwest::Client,
) -> Result<Vec<Vec<f32>>, EmbedError> {
    match cfg.provider.as_str() {
        "ollama" => embed_ollama(texts, cfg, http).await,
        "openai" | "custom" => embed_openai_compat(texts, cfg, http).await,
        p => Err(EmbedError(format!("unknown embed provider '{p}'; use ollama, openai, or custom"))),
    }
}

// ── Ollama ────────────────────────────────────────────────────────────────────
// Send one text at a time — Ollama ≥0.1.36 concatenates batch inputs
// which blows the context window on longer documents.

async fn embed_ollama(
    texts: &[String],
    cfg: &EmbedConfig,
    http: &reqwest::Client,
) -> Result<Vec<Vec<f32>>, EmbedError> {
    let mut results: Vec<Vec<f32>> = Vec::with_capacity(texts.len());

    for text in texts {
        // Truncate to ~6000 chars to stay inside any model's context window
        let safe = if text.len() > 6000 { &text[..6000] } else { text.as_str() };

        // Try /api/embed first (Ollama ≥0.1.36)
        let body = serde_json::json!({ "model": cfg.model, "input": safe });
        let res = http
            .post(format!("{}/api/embed", cfg.url))
            .json(&body)
            .send()
            .await
            .map_err(|e| EmbedError(format!("ollama /api/embed: {e}")))?;

        if res.status() == 404 {
            // Fall back to /api/embeddings (Ollama <0.1.36)
            let body2 = serde_json::json!({ "model": cfg.model, "prompt": safe });
            let res2 = http
                .post(format!("{}/api/embeddings", cfg.url))
                .json(&body2)
                .send()
                .await
                .map_err(|e| EmbedError(format!("ollama /api/embeddings: {e}")))?;
            if !res2.status().is_success() {
                return Err(EmbedError(format!("ollama /api/embeddings HTTP {}", res2.status())));
            }
            let v: Value = res2.json().await.map_err(|e| EmbedError(e.to_string()))?;
            let vec = parse_f32_array(&v["embedding"])
                .ok_or_else(|| EmbedError("ollama: missing 'embedding' field".into()))?;
            results.push(vec);
        } else {
            if !res.status().is_success() {
                let status = res.status();
                let body_txt = res.text().await.unwrap_or_default();
                return Err(EmbedError(format!("ollama /api/embed HTTP {status}: {body_txt}")));
            }
            let v: Value = res.json().await.map_err(|e| EmbedError(e.to_string()))?;
            // { embeddings: [[f32]] }
            let arr = v["embeddings"].as_array()
                .and_then(|a| a.first())
                .ok_or_else(|| EmbedError("ollama: empty 'embeddings' array".into()))?;
            let vec = parse_f32_array(arr)
                .ok_or_else(|| EmbedError("ollama: embeddings[0] is not a float array".into()))?;
            results.push(vec);
        }
    }
    Ok(results)
}

// ── OpenAI-compatible (OpenAI, custom) ───────────────────────────────────────
// Batches all texts in one request — OpenAI supports up to 2048 inputs.

async fn embed_openai_compat(
    texts: &[String],
    cfg: &EmbedConfig,
    http: &reqwest::Client,
) -> Result<Vec<Vec<f32>>, EmbedError> {
    // For custom: if VALORI_EMBED_URL already ends in a path, use as-is.
    // Otherwise append /v1/embeddings (OpenAI canonical).
    let endpoint = if cfg.url.contains("/v1/") || cfg.url.ends_with("/embed") {
        cfg.url.clone()
    } else {
        format!("{}/v1/embeddings", cfg.url)
    };

    let body = serde_json::json!({
        "model": cfg.model,
        "input": texts,
    });

    let mut req = http.post(&endpoint).json(&body);
    if let Some(ref key) = cfg.api_key {
        req = req.bearer_auth(key);
    }

    let res = req.send().await
        .map_err(|e| EmbedError(format!("embed POST {endpoint}: {e}")))?;

    if !res.status().is_success() {
        let status = res.status();
        let txt = res.text().await.unwrap_or_default();
        return Err(EmbedError(format!("embed {status}: {txt}")));
    }

    let v: Value = res.json().await.map_err(|e| EmbedError(e.to_string()))?;

    // Standard OpenAI shape: { data: [ { embedding: [f32] }, ... ] }
    if let Some(data) = v["data"].as_array() {
        return data.iter().map(|item| {
            parse_f32_array(&item["embedding"])
                .ok_or_else(|| EmbedError("openai: missing embedding in data item".into()))
        }).collect();
    }

    // Fallback: { embeddings: [[f32]] }
    if let Some(arr) = v["embeddings"].as_array() {
        return arr.iter().map(|item| {
            parse_f32_array(item)
                .ok_or_else(|| EmbedError("custom: embeddings item is not a float array".into()))
        }).collect();
    }

    Err(EmbedError(format!("embed: unexpected response shape: {}", v)))
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn parse_f32_array(v: &Value) -> Option<Vec<f32>> {
    v.as_array().map(|a| {
        a.iter().filter_map(|x| x.as_f64().map(|f| f as f32)).collect()
    })
}
