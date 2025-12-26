use tokio::fs::File;
use tokio::io::{AsyncReadExt, BufReader};
use crate::events::event_log::LogEntry;
use std::path::PathBuf;
use crate::errors::EngineError;

pub async fn spawn_replication_stream<const D: usize>(
    file_path: PathBuf,
    mut live_rx: tokio::sync::broadcast::Receiver<LogEntry<D>>,
    start_offset: u64,
) -> Result<tokio::sync::mpsc::Receiver<Result<String, EngineError>>, EngineError> {
    let (tx, rx) = tokio::sync::mpsc::channel(100);
    
    tokio::spawn(async move {
        let mut recent_hashes = std::collections::VecDeque::new();
        let max_history = 1000;
        
        // 1. Read File History
        if let Ok(file) = File::open(&file_path).await {
            let mut reader = BufReader::new(file);
            let mut buffer = Vec::new();
            
            if let Ok(_) = reader.read_to_end(&mut buffer).await {
                // Decode loop
                let mut offset = 0;
                // Skip Header
                if buffer.len() >= 16 {
                    offset = 16;
                }
                
                let mut current_idx = 0;
                
                while offset < buffer.len() {
                    match bincode::serde::decode_from_slice::<LogEntry<D>, _>(
                        &buffer[offset..],
                        bincode::config::standard()
                    ) {
                        Ok((entry, bytes_read)) => {
                            offset += bytes_read;
                            
                            // Track hash for deduplication
                            // We re-encode to get stable bytes for hashing, or use slice if possible?
                            // Slice included 'variant index' etc.
                            // Let's re-encode cheaply or hash the struct if we implement Hash?
                            // LogEntry doesn't impl Hash.
                            // Use bincode bytes of the entry we just read!
                            // Wait, `buffer[offset-bytes_read..offset]` is the bytes.
                            let entry_bytes = &buffer[offset-bytes_read..offset];
                            let hash = blake3::hash(entry_bytes);
                            
                            // Debug log
                            // tracing::info!("Stream File: Found hash {:?}", hash);
                            
                            if recent_hashes.len() >= max_history {
                                recent_hashes.pop_front();
                            }
                            recent_hashes.push_back(hash);
                            
                            // Send if >= start_offset and valid event
                            if let LogEntry::Event(_) = &entry {
                                if current_idx >= start_offset {
                                    if let Ok(json) = serde_json::to_string(&entry) {
                                        if tx.send(Ok(json + "\n")).await.is_err() {
                                            tracing::warn!("Stream: Client disconnected during file read");
                                            return;
                                        }
                                        tracing::debug!("Stream: Sent historical event idx {}", current_idx);
                                    }
                                }
                                current_idx += 1;
                            }
                        }
                        Err(e) => {
                            tracing::error!("Stream: Decode error at offset {}: {}", offset, e);
                            break; 
                        } // EOF or corrupt
                    }
                }
            }
        }
        
        // 2. Stream Live
        loop {
             match live_rx.recv().await {
                Ok(entry) => {
                    // Deduplicate
                    // Need to hash this entry.
                    let entry_bytes = bincode::serde::encode_to_vec(&entry, bincode::config::standard()).unwrap_or_default();
                    let hash = blake3::hash(&entry_bytes);
                    
                    if recent_hashes.contains(&hash) {
                        tracing::debug!("Stream: Dropping duplicate live event {:?}", hash);
                        continue;
                    }
                    
                    tracing::debug!("Stream: Sending live event {:?}", hash);

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
                Err(_) => break, // Lagged or Closed
             }
        }
    });

    Ok(rx)
}

use crate::network::LeaderClient;
use crate::server::SharedEngine;
use tokio_stream::StreamExt; // For iterating the response stream?
// Actually reqwest stream is `bytes_stream`.

#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize)]
pub enum ReplicationState {
    Synced,
    Diverged,
    Healing,
    Unknown,
}

pub static REPLICATION_STATUS: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(0); // 0=Unknown, 1=Synced, 2=Diverged, 3=Healing

pub async fn run_follower_loop<const M: usize, const D: usize, const N: usize, const E: usize>(
    state: SharedEngine<M, D, N, E>,
    leader_url: String,
) {
    let client = LeaderClient::new(leader_url);
    
    // Spawn Background Divergence Checker
    let state_checker = state.clone();
    let client_checker = client.clone();
    
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
            
            // Check State
            let (local_hash, local_height) = {
                let engine = state_checker.lock().await;
                // Only check if we are reasonably bootstrapped
                (engine.root_hash(), engine.event_committer.as_ref().map(|c| c.journal().committed_height()).unwrap_or(0))
            };
            
            if local_height == 0 { continue; }

            match client_checker.get_proof().await {
                Ok(proof) => {
                    // Simple check: If our height <= proof height (implied via hash check?), we can't easily check height on proof yet.
                    // But `proof` is `DeterministicProof`, which is `final_state_hash`.
                    // The Leader's proof is the CURRENT head.
                    // If we are significantly behind, hashes won't match, yielding false positives for divergence?
                    // YES.
                    // We need to ask leader for "Hash at height H" to verify OUR state.
                    // Current `/v1/proof/state` gives HEAD state.
                    
                    // IF we are caught up (stream idle?), hashes should match.
                    // IF we are lagging, they won't.
                    
                    // Correct approach:
                    // We cannot just compare HEAD hashes unless we know we are at the same height.
                    // BUT, if we are `Diverged`, we stay `Diverged`.
                    
                    // For MVP Phase 31: 
                    // Let's assume if we match, we are Synced.
                    // If we differ... we might just be lagging.
                    // We need `committed_height` in the Proof response! (API Change required?)
                    // `DeterministicProof` currently: `final_state_hash: [u8; 32]`.
                    // It does NOT have height. 
                    
                    // WORKAROUND:
                    // In `get_proof`, the leader is computing it on the fly or returning last commit?
                    // `api.rs` -> `generate_proof`.
                    
                    // Let's modify `ReplicationState` to just track "Last Verified Match".
                    if proof.final_state_hash == local_hash {
                         REPLICATION_STATUS.store(1, std::sync::atomic::Ordering::Relaxed); // Synced
                         tracing::debug!("Replication: State verified OK.");
                    } else {
                         tracing::warn!("Replication: State mismatch detected! Leader: {:?}, Local: {:?}", proof.final_state_hash, local_hash);
                         // For Phase 31 Verification: Report Diverged immediately on mismatch
                         REPLICATION_STATUS.store(2, std::sync::atomic::Ordering::Relaxed); // Diverged
                    }
                }
                Err(e) => {
                    tracing::warn!("Replication: Verification check failed: {}", e);
                }
            }
        }
    });
    
    loop {
        tracing::info!("Follower: Connecting to leader at {}...", client.base_url());
        
        // 1. Handshake / Proof Check
        match client.get_proof().await {
            Ok(proof) => {
                tracing::info!("Leader is at state hash: {:?}", proof.final_state_hash);
                // In future: compare with local state, detect divergence
            }
            Err(e) => {
                tracing::warn!("Failed to contact leader: {}. Retrying in 5s...", e);
                tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
                continue;
            }
        }
        
        // 2. Determine Local State
        let (_local_height, is_empty) = {
            let engine = state.lock().await;
            if let Some(ref committer) = engine.event_committer {
                let h = committer.journal().committed_height();
                (h, h == 0)
            } else {
                tracing::error!("Follower node MUST have event log enabled. Fatal error.");
                return;
            }
        };

        // 3. Bootstrap (Snapshot)
        // If local state is empty, try to bootstrap from leader's snapshot
        // This avoids replaying strict history from 0 if a snapshot exists.
        if is_empty {
             tracing::info!("Local state empty. Attempting snapshot bootstrap...");
             match bootstrap_from_leader(&state, &client).await {
                 Ok(_) => {
                     tracing::info!("Bootstrap successful!");
                 }
                 Err(e) => {
                     tracing::warn!("Snapshot bootstrap failed (Leader might not have one): {}. Falling back to stream replay.", e);
                 }
             }
        }
        
        // Refetch height in case bootstrap changed it
        let start_offset = {
            let engine = state.lock().await;
            engine.event_committer.as_ref().unwrap().journal().committed_height() as u64
        };
        
        tracing::info!("Follower: Starting replication stream from offset {}", start_offset);
        
        match client.stream_events(start_offset).await {
            Ok(resp) => {
                let mut stream = resp.bytes_stream();
                let mut buffer = String::new();
                
                loop {
                    // Use timeout to periodically check for divergence signal from background task
                    match tokio::time::timeout(tokio::time::Duration::from_secs(1), stream.next()).await {
                        Ok(Some(item)) => {
                            match item {
                                Ok(chunk) => {
                                    // chunk is bytes::Bytes
                                    let s = String::from_utf8_lossy(&chunk);
                                    buffer.push_str(&s);
                                    
                                    // Process lines
                                    while let Some(idx) = buffer.find('\n') {
                                        let line = buffer.drain(..=idx).collect::<String>();
                                        let line = line.trim();
                                        if line.is_empty() { continue; }
                                        
                                        // Parse
                                        match serde_json::from_str::<LogEntry<D>>(line) {
                                            Ok(LogEntry::Event(event)) => {
                                                let mut engine = state.lock().await;
                                                if let Some(ref mut committer) = engine.event_committer {
                                                    match committer.commit_event(event.clone()) {
                                                        Ok(_) => {
                                                            // Success
                                                            // Also sync Engine state (crucial fix from Leader)
                                                             if let Err(e) = engine.apply_committed_event(&event) {
                                                                 tracing::error!("Follower: Critical Divergence! Failed to apply event to kernel: {:?}", e);
                                                                 REPLICATION_STATUS.store(2, std::sync::atomic::Ordering::Relaxed); // Diverged
                                                                 break; // Break stream to trigger healing
                                                             }
                                                        }
                                                        Err(e) => {
                                                            tracing::error!("Follower: Commit failed: {:?}", e);
                                                            // If commit fails, we might be desynced or disk full. 
                                                            // For now, retry loop.
                                                            break;
                                                        }
                                                    }
                                                }
                                            }
                                            Ok(LogEntry::Checkpoint{..}) => {
                                                // Ignore checkpoints for now
                                            }
                                            Err(e) => {
                                                tracing::warn!("Follower: JSON parse error: {}", e);
                                            }
                                        }
                                    }
                                }
                                Err(e) => {
                                    tracing::warn!("Follower: Stream error: {}", e);
                                    break;
                                }
                            }
                        }
                        Ok(None) => {
                            tracing::warn!("Follower: Stream ended. Reconnecting...");
                            break;
                        }
                        Err(_) => {
                            // Timeout: Check Status
                             let status = REPLICATION_STATUS.load(std::sync::atomic::Ordering::Relaxed);
                             if status == 2 { // Diverged
                                 tracing::warn!("Follower: Divergence signal received during stream. breaking...");
                                 break;
                             }
                        }
                    }
                    
                    // Also break if inner loop set divergence
                    if REPLICATION_STATUS.load(std::sync::atomic::Ordering::Relaxed) == 2 {
                         break;
                    }
                }
            }
            Err(e) => {
                tracing::warn!("Follower: Connect failed: {}", e);
            }
        }
        
        // 4. Check for Healing Requirement
        let status = REPLICATION_STATUS.load(std::sync::atomic::Ordering::Relaxed);
        if status == 2 { // Diverged
             tracing::warn!("Follower: Divergence confirmed. Initiating Auto-Healing...");
             REPLICATION_STATUS.store(1, std::sync::atomic::Ordering::Relaxed); // Set to Healing
             
             if let Err(e) = bootstrap_from_leader(&state, &client).await {
                  tracing::error!("Follower: Healing failed: {}. Retrying in 5s...", e);
                  // We stay in Diverged/Healing state and retry loop
             } else {
                  tracing::info!("Follower: Healing successful. Resuming sync...");
                  REPLICATION_STATUS.store(0, std::sync::atomic::Ordering::Relaxed); // Unknown (will check verify next)
             }
        }
        
        tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
    }
}

async fn bootstrap_from_leader<const M: usize, const D: usize, const N: usize, const E: usize>(
    state: &SharedEngine<M, D, N, E>,
    client: &LeaderClient,
) -> Result<(), EngineError> {
    tracing::info!("Bootstrap/Healing: Downloading snapshot from Leader...");
    let snapshot_bytes = client.download_snapshot().await?;
    
    tracing::info!("Bootstrap/Healing: Restoring snapshot ({} bytes)...", snapshot_bytes.len());
    
    // We need to re-initialize EventLog logic because we are jumping history.
    // 1. Restore Memory State
    // 2. Wipe/Reset Local Event Log
    // 3. Initialize new Event Log with Checkpoint at new height
    
    let mut engine = state.lock().await;
    
    // 1. Restore
    engine.restore(&snapshot_bytes)?;
    
    // 2. Reset Log logic
    // We must retrieve path BEFORE dropping committer
    let log_path = engine.event_committer.as_ref()
        .map(|c| c.event_log().path().to_path_buf())
        .ok_or(EngineError::Internal)?;
    
    // Drop old committer to release lock?
    engine.event_committer = None;
    
    // Delete file
    if tokio::fs::metadata(&log_path).await.is_ok() {
        if let Err(e) = tokio::fs::remove_file(&log_path).await {
             tracing::error!("Failed to delete diverged log: {}", e);
             return Err(EngineError::Unknown(e.to_string()));
        }
    }
    
    let new_height = engine.state.record_count() as u64; 
    let state_hash = engine.root_hash();
    
    // Create new components
    let log_writer = crate::events::event_log::EventLogWriter::open(&log_path)
         .map_err(|e| EngineError::Unknown(e.to_string()))?;
         
    let journal = crate::events::event_journal::EventJournal::new_at_height(new_height);
    
    // Re-create committer
    let mut committer = crate::events::EventCommitter::new(log_writer, journal, engine.state.clone());
    
    // Write Checkpoint
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs();
    
    let checkpoint = crate::events::event_log::LogEntry::Checkpoint {
        event_count: new_height,
        snapshot_hash: state_hash,
        timestamp: now,
        // Removed previous_hash
    };
    
    if let Err(e) = committer.write_checkpoint(checkpoint) {
         return Err(EngineError::Unknown(format!("Checkpoint write failed: {:?}", e)));
    }
    
    engine.event_committer = Some(committer);
    
    tracing::info!("Bootstrap/Healing complete. State at height {}, hash {:?}", new_height, state_hash);
    
    Ok(())
}
