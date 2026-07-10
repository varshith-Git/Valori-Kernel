// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! `make-demo-log` — generate a deterministic event log for the tamper demo.
//!
//! Writes a real `events.log` (v2 wire format, identical to a production node)
//! containing N InsertRecord events plus a small knowledge graph, then prints
//! the expected BLAKE3 state hash to stdout. Pseudo-random values come from a
//! fixed-seed LCG so the same arguments always produce the same file and hash.
//!
//! ```text
//! make-demo-log /tmp/demo/events.log 2000
//! ```

use std::io::Write;
use std::process::ExitCode;

use valori_kernel::event::KernelEvent;
use valori_kernel::snapshot::blake3::hash_state_blake3;
use valori_kernel::state::kernel::KernelState;
use valori_kernel::types::enums::{EdgeKind, NodeKind};
use valori_kernel::types::id::{EdgeId, NodeId, RecordId};
use valori_kernel::types::scalar::FxpScalar;
use valori_kernel::types::vector::FxpVector;

use valori_wire::{
    chain_advance_v3, encode_entry, encode_header_v4, hex, LogEntry, FORMAT_Q16_16, VERSION_V4,
};

const DIM: usize = 4;
/// Simulated base timestamp for demo events so the output looks realistic.
const BASE_TIMESTAMP: u64 = 1_750_000_000; // ~2025-06-15

/// Deterministic LCG (same constants as glibc) — keeps the demo reproducible.
struct Lcg(u64);

impl Lcg {
    fn next(&mut self) -> u32 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        (self.0 >> 33) as u32
    }

    fn fxp(&mut self) -> FxpScalar {
        let raw = (self.next() % 131072) as i32 - 65536;
        FxpScalar(raw)
    }
}

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    let (path, count) = match (args.next(), args.next()) {
        (Some(p), Some(c)) => match c.parse::<u32>() {
            Ok(n) if n > 0 => (p, n),
            _ => {
                eprintln!("error: COUNT must be a positive integer, got '{c}'");
                return ExitCode::from(2);
            }
        },
        _ => {
            eprintln!("usage: make-demo-log <PATH> <COUNT>");
            eprintln!("       writes COUNT InsertRecord events plus a small graph to PATH");
            return ExitCode::from(2);
        }
    };

    let mut rng = Lcg(0x5EED_CAFE);
    let mut events: Vec<KernelEvent> = Vec::new();

    for i in 0..count {
        let data: Vec<FxpScalar> = (0..DIM).map(|_| rng.fxp()).collect();
        events.push(KernelEvent::InsertRecord {
            id: RecordId(i),
            vector: FxpVector { data },
            metadata: None,
            tag: 0,
        });
    }

    // A small knowledge graph on top: 4 nodes, 3 edges
    for i in 0..4u32 {
        events.push(KernelEvent::CreateNode {
            id: NodeId(i),
            kind: NodeKind::Concept,
            record: if i < 2 { Some(RecordId(i)) } else { None },
        });
    }
    for (i, (from, to)) in [(0u32, 1u32), (1, 2), (2, 3)].iter().enumerate() {
        events.push(KernelEvent::CreateEdge {
            id: EdgeId(i as u32),
            kind: EdgeKind::Relation,
            from: NodeId(*from),
            to: NodeId(*to),
        });
    }

    // Replay locally to compute the expected state hash.
    let mut state = KernelState::new();
    for evt in &events {
        if let Err(e) = state.apply_event(evt) {
            eprintln!("internal error: demo event rejected by kernel: {e:?}");
            return ExitCode::from(2);
        }
    }
    let expected = hex(&hash_state_blake3(&state));

    // Write the log in the current (v3) wire format.
    let file = match std::fs::File::create(&path) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("error: cannot create '{path}': {e}");
            return ExitCode::from(2);
        }
    };
    let mut out = std::io::BufWriter::new(file);

    if let Err(e) = out.write_all(&encode_header_v4(DIM as u32, FORMAT_Q16_16, 0, &[0u8; 32])) {
        eprintln!("error: write failed: {e}");
        return ExitCode::from(2);
    }

    let mut chain_head = [0u8; 32];
    for (idx, evt) in events.iter().enumerate() {
        let wall_time_secs = BASE_TIMESTAMP + idx as u64;
        let log_entry = LogEntry::Event(evt.clone());
        let bytes = match encode_entry(VERSION_V4, &chain_head, wall_time_secs, None, &log_entry) {
            Ok(b) => b,
            Err(e) => {
                eprintln!("error: encode failed: {e}");
                return ExitCode::from(2);
            }
        };
        if let Err(e) = out.write_all(&bytes) {
            eprintln!("error: write failed: {e}");
            return ExitCode::from(2);
        }
        chain_head = chain_advance_v3(&chain_head, wall_time_secs, None, &log_entry);
    }

    if let Err(e) = out.flush() {
        eprintln!("error: flush failed: {e}");
        return ExitCode::from(2);
    }

    let total_events = count as usize + 7; // N records + 4 nodes + 3 edges
    eprintln!("wrote {total_events} events to {path}");
    // stdout carries ONLY the hash so scripts can capture it.
    println!("{expected}");
    ExitCode::SUCCESS
}
