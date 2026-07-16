// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! Minimal LLM HTTP client for entity extraction.
//!
//! Decouples the community layer from `valori-node`'s `EmbedConfig` — callers
//! construct a `LlmConfig` from whatever credentials they hold; this module
//! knows only how to talk to OpenAI-compatible and Ollama endpoints.

use crate::community::LlmExtractionOutput;

/// Credentials and routing info for a single LLM provider.
/// Mirrors the 4 fields of `valori-node`'s `EmbedConfig` that entity extraction needs.
#[derive(Clone, Debug)]
pub struct LlmConfig {
    pub provider: String,
    pub model: String,
    pub url: String,
    pub api_key: Option<String>,
}

/// Call the configured provider's chat/completion endpoint to extract entities
/// and relationships from `text`.
///
/// Supports `"openai"` / `"custom"` (OpenAI-compatible `/chat/completions`) and
/// `"ollama"` (`/api/generate`). Returns an error string for any other provider.
pub async fn extract_entities_via_llm(
    text: &str,
    entity_types: &[String],
    cfg: &LlmConfig,
    model_override: Option<&str>,
    http: &reqwest::Client,
) -> Result<LlmExtractionOutput, String> {
    let default_types = ["PERSON", "ORGANIZATION", "CONCEPT", "LOCATION", "EVENT"];
    let types_str = if entity_types.is_empty() {
        default_types.join(", ")
    } else {
        entity_types.join(", ")
    };

    let prompt = format!(
        r#"You are an entity extraction system. Extract entities and relationships from the text below.

Entity types to extract: {types}

Return a JSON object with exactly this structure (no extra keys, no markdown):
{{
  "entities": [
    {{"name": "EntityName", "type": "ENTITY_TYPE", "description": "Brief factual description"}}
  ],
  "relationships": [
    {{"source": "EntityName1", "target": "EntityName2", "description": "relationship description", "strength": 0.8}}
  ]
}}

TEXT:
{text}

JSON:"#,
        types = types_str,
        text = text,
    );

    match cfg.provider.as_str() {
        "openai" | "custom" => {
            let base = cfg.url.trim_end_matches('/');
            let url = format!("{base}/chat/completions");
            let model = model_override.unwrap_or_else(|| {
                if cfg.model.contains("embed") { "gpt-4o-mini" } else { &cfg.model }
            });
            let body = serde_json::json!({
                "model": model,
                "messages": [{"role": "user", "content": prompt}],
                "temperature": 0,
                "response_format": {"type": "json_object"}
            });
            let mut req = http.post(&url).json(&body);
            if let Some(ref key) = cfg.api_key {
                req = req.bearer_auth(key);
            }
            let resp = req.send().await.map_err(|e| e.to_string())?;
            let json: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
            let content = json["choices"][0]["message"]["content"]
                .as_str()
                .ok_or_else(|| "no content in LLM response".to_string())?;
            serde_json::from_str(content).map_err(|e| format!("JSON parse error: {e}"))
        }
        "ollama" => {
            let base = cfg.url.trim_end_matches('/');
            let url = format!("{base}/api/generate");
            let model = model_override.unwrap_or(&cfg.model);
            let body = serde_json::json!({
                "model": model,
                "prompt": prompt,
                "format": "json",
                "stream": false
            });
            let resp = http.post(&url).json(&body).send().await.map_err(|e| e.to_string())?;
            let json: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
            let content = json["response"]
                .as_str()
                .ok_or_else(|| "no response field from Ollama".to_string())?;
            serde_json::from_str(content).map_err(|e| format!("JSON parse error: {e}"))
        }
        other => Err(format!("entity extraction not supported for provider '{other}'")),
    }
}
