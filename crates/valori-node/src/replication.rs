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
                let (mut offset, log_version) = match valori_wire::parse_header(&buffer) {
                    Ok(h) => (h.header_len, h.version),
                    // Empty/invalid file → skip the file-replay phase.
                    Err(_) => (buffer.len(), valori_wire::VERSION_V3),
                };

                let mut current_idx = 0;

                while offset < buffer.len() {
                    match valori_wire::decode_entry(log_version, &buffer[offset..]) {
                        Ok((chained, bytes_read)) => {
                            offset += bytes_read;
                            // Re-encode only the inner LogEntry for the wire — the
                            // follower applies LogEntry, not the on-disk entry.
                            let entry_bytes = match bincode::serde::encode_to_vec(
                                &chained.entry,
                                bincode::config::standard(),
                            ) {
                                Ok(b) => b,
                                Err(_) => break,
                            };
                            let hash = blake3::hash(&entry_bytes);

                            if recent_hashes.len() >= max_history {
                                recent_hashes.pop_front();
                            }
                            recent_hashes.push_back(hash);

                            // S15: stream both plain and namespace-scoped data
                            // events (checkpoints/admin are not replayed here).
                            if matches!(&chained.entry, LogEntry::Event(_) | LogEntry::EventNs { .. }) {
                                if current_idx >= start_offset {
                                    use base64::{Engine as _, engine::general_purpose::STANDARD};
                                    let b64 = STANDARD.encode(&entry_bytes);
                                    let json = format!(r#"{{"b64":"{}"}}"#, b64);
                                    if tx.send(Ok(json + "\n")).await.is_err() {
                                        return;
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

                    use base64::{Engine as _, engine::general_purpose::STANDARD};
                    let b64 = STANDARD.encode(&entry_bytes);
                    let json = format!(r#"{{"b64":"{}"}}"#, b64);
                    if tx.send(Ok(json + "\n")).await.is_err() {
                        return;
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

/// Watch channel for replication state — one writer (hash-checker), one reader
/// (stream loop).  Replaces the old bare AtomicU8 which had no coordination
/// between the two tasks.
pub type ReplicationStateWatch = tokio::sync::watch::Receiver<ReplicationState>;

/// Global last-known replication state for the HTTP status endpoint.
/// Written only by the hash-checker task; read only by the HTTP handler.
/// An AtomicU8 is fine here because this is a *display* value, not a
/// coordination signal — the watch channel handles coordination.
static DISPLAY_STATUS: std::sync::atomic::AtomicU8 =
    std::sync::atomic::AtomicU8::new(0);

pub fn replication_display_state() -> &'static str {
    match DISPLAY_STATUS.load(std::sync::atomic::Ordering::Relaxed) {
        1 => "Synced",
        2 => "Diverged",
        3 => "Healing",
        _ => "Unknown",
    }
}

pub async fn run_follower_loop(
    state: SharedEngine,
    leader_url: String,
) {
    let client = LeaderClient::new(leader_url);

    // Single writer; stream loop only reads.
    let (status_tx, mut status_rx) = tokio::sync::watch::channel(ReplicationState::Unknown);

    let state_checker = state.clone();
    let client_checker = client.clone();

    tokio::spawn(async move {
        loop {
            tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;

            let (local_hash, local_height) = {
                let engine = state_checker.read().await;
                let h = engine.get_proof().final_state_hash;
                let hex: String = h.iter().map(|b| format!("{b:02x}")).collect();
                (
                    hex,
                    engine.event_committer()
                        .map(|c| c.journal().committed_height())
                        .unwrap_or(0),
                )
            };

            if local_height == 0 { continue; }

            match client_checker.get_proof().await {
                Ok(proof) => {
                    let new_state = if proof.final_state_hash == local_hash {
                        DISPLAY_STATUS.store(1, std::sync::atomic::Ordering::Relaxed);
                        ReplicationState::Synced
                    } else {
                        DISPLAY_STATUS.store(2, std::sync::atomic::Ordering::Relaxed);
                        ReplicationState::Diverged
                    };
                    // send() only errors if all receivers are dropped — ignore.
                    let _ = status_tx.send(new_state);
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
            let engine = state.read().await;
            if let Some(committer) = engine.event_committer() {
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
            let engine = state.read().await;
            engine.event_committer().unwrap().journal().committed_height() as u64
        };

        // Mark the watch as seen before entering the stream loop so we only
        // react to divergence signals that arrive *during* this loop iteration.
        status_rx.borrow_and_update();

        if let Ok(resp) = client.stream_events(start_offset).await {
            let mut stream = resp.bytes_stream();
            let mut buffer = String::new();
            let mut apply_failed = false;

            'stream: loop {
                // Check for divergence signal from hash-checker without blocking.
                if status_rx.has_changed().unwrap_or(false) {
                    let s = *status_rx.borrow_and_update();
                    if s == ReplicationState::Diverged {
                        apply_failed = true;
                        break 'stream;
                    }
                }

                match tokio::time::timeout(
                    tokio::time::Duration::from_secs(1),
                    stream.next(),
                ).await {
                    Ok(Some(Ok(chunk))) => {
                        let s = String::from_utf8_lossy(&chunk);
                        tracing::debug!("Follower received chunk from stream: {}", s);
                        buffer.push_str(&s);

                        while let Some(idx) = buffer.find('\n') {
                            let line = buffer.drain(..=idx).collect::<String>();
                            let line = line.trim();
                            if line.is_empty() { continue; }
                            
                            #[derive(serde::Deserialize)]
                            struct B64Message {
                                b64: String,
                            }
                            
                            if let Ok(msg) = serde_json::from_str::<B64Message>(line) {
                                use base64::{Engine as _, engine::general_purpose::STANDARD};
                                if let Ok(bytes) = STANDARD.decode(&msg.b64) {
                                    // S15: preserve the namespace across the wire so a
                                    // replicated collection write lands in the same
                                    // collection on the follower.
                                    let decoded = bincode::serde::decode_from_slice::<LogEntry, _>(&bytes, bincode::config::standard())
                                        .ok()
                                        .map(|(e, _)| e);
                                    let ns_event = match decoded {
                                        Some(LogEntry::Event(event)) => Some((valori_kernel::types::id::DEFAULT_NS.0, event)),
                                        Some(LogEntry::EventNs { namespace_id, event }) => Some((namespace_id, event)),
                                        _ => None,
                                    };
                                    if let Some((namespace_id, event)) = ns_event {
                                        let mut engine = state.write().await;
                                        if let Some(committer) = engine.event_committer_mut() {
                                            match committer.commit_event_ns(event.clone(), namespace_id) {
                                                Ok(_) => {
                                                    if let Err(e) = engine.apply_committed_event_ns(&event, namespace_id) {
                                                        tracing::error!("Failed to apply committed event: {:?}", e);
                                                        apply_failed = true;
                                                        break 'stream;
                                                    }
                                                    tracing::debug!("Successfully applied event to follower index");
                                                }
                                                Err(e) => {
                                                    tracing::error!("Follower failed to commit event: {:?}", e);
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                    Ok(Some(Err(_))) | Ok(None) => break 'stream,
                    // Timeout — continue to re-check divergence signal.
                    Err(_) => {}
                }
            }

            if apply_failed {
                // Acquire engine lock here — hash-checker task never holds it,
                // so there is no lock-ordering issue.
                let _ = status_tx_heal(&state, &client).await;
            }
        }

        tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
    }
}

/// Separate function so the healing path is clear and testable.
async fn status_tx_heal(state: &SharedEngine, client: &LeaderClient) -> Result<(), EngineError> {
    tracing::warn!("Replication divergence detected — bootstrapping from leader");
    DISPLAY_STATUS.store(3, std::sync::atomic::Ordering::Relaxed); // Healing
    let result = bootstrap_from_leader(state, client).await;
    if result.is_ok() {
        DISPLAY_STATUS.store(1, std::sync::atomic::Ordering::Relaxed); // Synced
    }
    result
}

async fn bootstrap_from_leader(
    state: &SharedEngine,
    client: &LeaderClient,
) -> Result<(), EngineError> {
    let snapshot_bytes = client.download_snapshot().await?;
    let mut engine = state.write().await;
    engine.restore(&snapshot_bytes)?;

    let log_path = engine.event_committer()
        .map(|c| c.event_log().path().to_path_buf())
        .ok_or(EngineError::InvalidInput("No event log path".to_string()))?;

    let dim = engine.event_committer().map(|c| c.event_log().dim());
    engine.persistence = crate::commit::Persistence::Ephemeral;

    let _ = tokio::fs::remove_file(&log_path).await;

    let new_height = engine.state.record_count() as u64;
    let state_hash = engine.get_proof().final_state_hash;

    let log_writer = crate::events::event_log::EventLogWriter::open(&log_path, dim)
        .map_err(|e| EngineError::InvalidInput(e.to_string()))?;

    let journal = crate::events::event_journal::EventJournal::new_at_height(new_height);
    let mut committer = crate::events::event_commit::EventCommitter::new(
        log_writer, journal, engine.state.clone(),
    );

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();

    let checkpoint = crate::events::event_log::LogEntry::Checkpoint {
        event_count: new_height,
        snapshot_hash: state_hash,
        timestamp: now,
    };

    committer.write_checkpoint(checkpoint)
        .map_err(|e| EngineError::InvalidInput(format!("{:?}", e)))?;
    engine.persistence = crate::commit::Persistence::EventLog(committer);

    tracing::info!("Bootstrap complete — follower synced at height {}", new_height);
    Ok(())
}
