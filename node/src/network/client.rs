use crate::errors::EngineError;
use reqwest::Client;

#[derive(Debug, Clone)]
pub struct LeaderClient {
    base_url: String,
    client: Client,
}

impl LeaderClient {
    pub fn new(url: String) -> Self {
        Self {
            base_url: url.trim_end_matches('/').to_string(),
            client: Client::new(),
        }
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    pub async fn get_proof(&self) -> Result<valori_kernel::proof::DeterministicProof, EngineError> {
        let url = format!("{}/v1/proof/state", self.base_url);
        let resp = self.client.get(&url).send().await
            .map_err(|e| EngineError::Network(e.to_string()))?;
            
        if !resp.status().is_success() {
            return Err(EngineError::Network(format!("Proof request failed: {}", resp.status())));
        }
        
        resp.json().await.map_err(|e| EngineError::Network(e.to_string()))
    }
    
    // We stream bytes for events to handle NDJSON manually or use a line streamer
    pub async fn stream_events(&self, start_offset: u64) -> Result<reqwest::Response, EngineError> {
        let url = format!("{}/v1/replication/events?start_offset={}", self.base_url, start_offset);
        let resp = self.client.get(&url).send().await
            .map_err(|e| EngineError::Network(e.to_string()))?;
            
        if !resp.status().is_success() {
            return Err(EngineError::Network(format!("Stream request failed: {}", resp.status())));
        }
        
        Ok(resp)
    }
    
    pub async fn download_snapshot(&self) -> Result<Vec<u8>, EngineError> {
        let url = format!("{}/v1/snapshot/download", self.base_url);
        let resp = self.client.get(&url).send().await
            .map_err(|e| EngineError::Network(e.to_string()))?;
            
        if !resp.status().is_success() {
            return Err(EngineError::Network(format!("Snapshot request failed: {}", resp.status())));
        }
        
        let bytes = resp.bytes().await.map_err(|e| EngineError::Network(e.to_string()))?;
        Ok(bytes.to_vec())
    }
}
