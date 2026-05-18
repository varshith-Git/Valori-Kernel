use anyhow::{Context, Result};

use valori_kernel::ValoriKernel;
use valori_persistence::snapshot;
use valori_node::events::event_log::LogEntry;

pub struct ForensicEngine {
    pub snapshot_index: u64,
    pub current_index: u64,
    pub state: ValoriKernel,
    pub applied_events: Vec<u64>,
}

impl ForensicEngine {
    pub fn new(snapshot_path: &str) -> Result<Self> {
        let (header, body) = snapshot::read_snapshot(snapshot_path)
            .context("Failed to read snapshot")?;
        
        let kernel = ValoriKernel::load_snapshot(&body)
            .map_err(|e| anyhow::anyhow!("Failed to load kernel from snapshot: {}", e))?;
        
        Ok(Self {
            snapshot_index: header.event_index,
            current_index: header.event_index,
            state: kernel,
            applied_events: Vec::new(),
        })
    }

    pub fn replay_to(&mut self, wal_path: &str, target_index: u64) -> Result<usize> {
        let mut replayed_count = 0;
        let file_bytes = std::fs::read(wal_path)
            .context("Failed to read events.log")?;
            
        if file_bytes.len() < 16 {
            return Ok(0); // Empty or corrupt log
        }

        let mut offset = 16;
        let mut virtual_eid = 0;

        while offset < file_bytes.len() {
            match bincode::serde::decode_from_slice::<LogEntry, _>(
                &file_bytes[offset..],
                bincode::config::standard()
            ) {
                Ok((entry, bytes_read)) => {
                    offset += bytes_read;
                    
                    match entry {
                        LogEntry::Event(event) => {
                            virtual_eid += 1;
                            
                            // 1. FILTER: Skip if already in snapshot
                            if virtual_eid <= self.snapshot_index {
                                continue;
                            }

                            // 3. STOP: If beyond target
                            if virtual_eid > target_index {
                                break;
                            }

                            // 2. REPLAY: Apply event
                            self.state.apply_event(&bincode::serde::encode_to_vec(&event, bincode::config::standard()).unwrap())
                                .map_err(|e| anyhow::anyhow!("Kernel Error at Event {}: {:?}", virtual_eid, e))?;

                            self.current_index = virtual_eid;
                            self.applied_events.push(virtual_eid);
                            replayed_count += 1;
                        }
                        LogEntry::Checkpoint { event_count, .. } => {
                            virtual_eid = event_count;
                        }
                    }
                }
                Err(e) => {
                    return Err(anyhow::anyhow!("WAL corrupt at offset {}: {}", offset, e));
                }
            }
        }
        
        Ok(replayed_count)
    }
}
