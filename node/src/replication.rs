use tokio::fs::File;
use tokio::io::{AsyncReadExt, BufReader};
use crate::events::event_log::LogEntry;
use std::path::PathBuf;
use crate::errors::EngineError;

pub async fn spawn_replication_stream(
    file_path: PathBuf,
    mut live_rx: tokio::sync::broadcast::Receiver<LogEntry>,
    start_offset: u64,
) -> Result<tokio::sync::mpsc::Receiver<Result<String, EngineError>>, EngineError> {
    let (tx, rx) = tokio::sync::mpsc::channel(100);
    
    tokio::spawn(async move {
        let mut recent_hashes = std::collections::VecDeque::new();
        let max_history = 1000;
        
        if let Ok(file) = File::open(&file_path).await {
            let mut reader = BufReader::new(file);
            let mut buffer = Vec::new();
            
            if let Ok(_) = reader.read_to_end(&mut buffer).await {
                let mut offset = 0;
                if buffer.len() >= 16 {
                    offset = 16;
                }
                
                let mut current_idx = 0;
                
                while offset < buffer.len() {
                    match bincode::serde::decode_from_slice::<LogEntry, _>(
                        &buffer[offset..],
                        bincode::config::standard()
                    ) {
                        Ok((entry, bytes_read)) => {
                            offset += bytes_read;
                            let entry_bytes = &buffer[offset-bytes_read..offset];
                            let hash = blake3::hash(entry_bytes);
                            
                            if recent_hashes.len() >= max_history {
                                recent_hashes.pop_front();
                            }
                            recent_hashes.push_back(hash);
                            
                            if let LogEntry::Event(_) = &entry {
                                if current_idx >= start_offset {
                                    if let Ok(json) = serde_json::to_string(&entry) {
                                        if tx.send(Ok(json + "\n")).await.is_err() {
                                            return;
                                        }
                                    }
                                }
                                current_idx += 1;
                            }
                        }
                        Err(_) => break,
                    }
                }
            }
        }
        
        loop {
             match live_rx.recv().await {
                Ok(entry) => {
                    let entry_bytes = bincode::serde::encode_to_vec(&entry, bincode::config::standard()).unwrap_or_default();
                    let hash = blake3::hash(&entry_bytes);
                    
                    if recent_hashes.contains(&hash) {
                        continue;
                    }
                    
                    if recent_hashes.len() >= max_history {
                         recent_hashes.pop_front();
                    }
                    recent_hashes.push_back(hash);
                    
                    if let Ok(json) = serde_json::to_string(&entry) {
                         if tx.send(Ok(json + "\n")).await.is_err() {
                             return;
                         }
                    }
                }
                Err(_) => break,
             }
        }
    });

    Ok(rx)
}

use crate::network::LeaderClient;
use crate::server::SharedEngine;
use tokio_stream::StreamExt;

#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize)]
pub enum ReplicationState {
    Synced,
    Diverged,
    Healing,
    Unknown,
}

pub static REPLICATION_STATUS: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(0);

pub async fn run_follower_loop(
    state: SharedEngine,
    leader_url: String,
) {
    let client = LeaderClient::new(leader_url);
    
    let state_checker = state.clone();
    let client_checker = client.clone();
    
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
            
            let (local_hash, local_height) = {
                let engine = state_checker.lock().await;
                (engine.get_proof().final_state_hash, engine.event_committer.as_ref().map(|c| c.journal().committed_height()).unwrap_or(0))
            };
            
            if local_height == 0 { continue; }

            match client_checker.get_proof().await {
                Ok(proof) => {
                    if proof.final_state_hash == local_hash {
                         REPLICATION_STATUS.store(1, std::sync::atomic::Ordering::Relaxed);
                    } else {
                         REPLICATION_STATUS.store(2, std::sync::atomic::Ordering::Relaxed);
                    }
                }
                Err(_) => {}
            }
        }
    });
    
    loop {
        match client.get_proof().await {
            Ok(_) => {}
            Err(_) => {
                tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
                continue;
            }
        }
        
        let (_local_height, is_empty) = {
            let engine = state.lock().await;
            if let Some(ref committer) = engine.event_committer {
                let h = committer.journal().committed_height();
                (h, h == 0)
            } else {
                return;
            }
        };

        if is_empty {
             let _ = bootstrap_from_leader(&state, &client).await;
        }
        
        let start_offset = {
            let engine = state.lock().await;
            engine.event_committer.as_ref().unwrap().journal().committed_height() as u64
        };
        
        if let Ok(resp) = client.stream_events(start_offset).await {
            let mut stream = resp.bytes_stream();
            let mut buffer = String::new();
            
            loop {
                match tokio::time::timeout(tokio::time::Duration::from_secs(1), stream.next()).await {
                    Ok(Some(item)) => {
                        match item {
                            Ok(chunk) => {
                                let s = String::from_utf8_lossy(&chunk);
                                buffer.push_str(&s);
                                
                                while let Some(idx) = buffer.find('\n') {
                                    let line = buffer.drain(..=idx).collect::<String>();
                                    let line = line.trim();
                                    if line.is_empty() { continue; }
                                    
                                    if let Ok(LogEntry::Event(event)) = serde_json::from_str::<LogEntry>(line) {
                                        let mut engine = state.lock().await;
                                        if let Some(ref mut committer) = engine.event_committer {
                                            if let Ok(_) = committer.commit_event(event.clone()) {
                                                 if let Err(_) = engine.apply_committed_event(&event) {
                                                     REPLICATION_STATUS.store(2, std::sync::atomic::Ordering::Relaxed);
                                                     break;
                                                 }
                                            }
                                        }
                                    }
                                }
                            }
                            Err(_) => break,
                        }
                    }
                    Ok(None) => break,
                    Err(_) => {
                         if REPLICATION_STATUS.load(std::sync::atomic::Ordering::Relaxed) == 2 {
                             break;
                         }
                    }
                }
            }
        }
        
        let status = REPLICATION_STATUS.load(std::sync::atomic::Ordering::Relaxed);
        if status == 2 {
             REPLICATION_STATUS.store(1, std::sync::atomic::Ordering::Relaxed);
             if let Ok(_) = bootstrap_from_leader(&state, &client).await {
                  REPLICATION_STATUS.store(0, std::sync::atomic::Ordering::Relaxed);
             }
        }
        
        tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
    }
}

async fn bootstrap_from_leader(
    state: &SharedEngine,
    client: &LeaderClient,
) -> Result<(), EngineError> {
    let snapshot_bytes = client.download_snapshot().await?;
    let mut engine = state.lock().await;
    engine.restore(&snapshot_bytes)?;
    
    let log_path = engine.event_committer.as_ref()
        .map(|c| c.event_log().path().to_path_buf())
        .ok_or(EngineError::InvalidInput("No event log path".to_string()))?;
    
    let dim = engine.event_committer.as_ref().map(|c| c.event_log().dim());
    engine.event_committer = None;
    
    let _ = tokio::fs::remove_file(&log_path).await;
    
    let new_height = engine.state.record_count() as u64; 
    let state_hash = engine.get_proof().final_state_hash;
    
    let log_writer = crate::events::event_log::EventLogWriter::open(&log_path, dim)
         .map_err(|e| EngineError::InvalidInput(e.to_string()))?;
         
    let journal = crate::events::event_journal::EventJournal::new_at_height(new_height);
    let mut committer = crate::events::event_commit::EventCommitter::new(log_writer, journal, engine.state.clone());
    
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs();
    
    let checkpoint = crate::events::event_log::LogEntry::Checkpoint {
        event_count: new_height,
        snapshot_hash: state_hash,
        timestamp: now,
    };
    
    committer.write_checkpoint(checkpoint).map_err(|e| EngineError::InvalidInput(format!("{:?}", e)))?;
    engine.event_committer = Some(committer);
    
    Ok(())
}
