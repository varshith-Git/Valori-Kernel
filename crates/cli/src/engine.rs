use anyhow::{Context, Result};

use valori_kernel::ValoriKernel;
use valori_persistence::{snapshot, wal};

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
        let reader = wal::read_stream(wal_path)
            .context("Failed to open WAL stream")?;

        for entry_result in reader {
            let entry = entry_result?;
            let eid = entry.header.event_id;

            // 1. FILTER: Skip if already in snapshot
            if eid <= self.snapshot_index {
                continue;
            }

            // 3. STOP: If beyond target
            if eid > target_index {
                break;
            }

            // 2. REPLAY: Apply event
            // FAIL-CLOSED: Any error from kernel stops replay immediately.
            self.state.apply_event(&entry.payload)
                .map_err(|e| anyhow::anyhow!("Kernel Error at Event {}: {}", eid, e))?;

            self.current_index = eid; // Set to strictly the last applied event ID
            self.applied_events.push(eid);
            replayed_count += 1;
        }
        
        // Graceful End: If we finish the loop (EOF) without reaching target_index,
        // we just stop. The calling code can check forensic_engine.current_index vs target_index if it cares.
        
        Ok(replayed_count)
    }
}
