// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
use clap::{Parser, Subcommand};
use std::path::PathBuf;
use valori_cli::commands::{diff, inspect, replay_query, timeline, verify};

#[derive(Parser)]
#[command(
    name    = "valori",
    version = env!("CARGO_PKG_VERSION"),
    author  = "Varshith Gudur",
    about   = "Valori Forensic CLI — black-box flight recorder for Valori AI memory databases",
    long_about = None,
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Inspect database files and print a status summary.
    ///
    /// Pass --dir to auto-resolve snapshot.val and events.log from a database
    /// directory, or supply individual file paths with --snapshot / --log.
    Inspect {
        /// Database directory (auto-resolves snapshot.val and events.log).
        #[arg(long, short)]
        dir: Option<PathBuf>,

        /// Path to the snapshot file (overrides --dir).
        #[arg(long)]
        snapshot: Option<String>,

        /// Path to the event log file (overrides --dir).
        #[arg(long)]
        log: Option<String>,
    },

    /// Verify the structural integrity and magic bytes of a snapshot file.
    Verify {
        /// Path to the snapshot file.
        snapshot: String,
    },

    /// Print the event timeline from an event log.
    Timeline {
        /// Path to the events.log file.
        log: String,

        /// Maximum number of events to display (0 = all).
        #[arg(long, default_value = "0")]
        limit: usize,
    },

    /// Fast-forward to a specific event count and report the database state.
    ///
    /// Restores the snapshot baseline, then replays events 1–N from the event
    /// log, and prints the state hash and optional search results at event N.
    ReplayQuery {
        /// Path to the snapshot file (baseline state).
        #[arg(long)]
        snapshot: String,

        /// Path to the event log file.
        #[arg(long)]
        log: String,

        /// Replay events 1–N and report the kernel state at event N.
        #[arg(long, short)]
        at: u64,

        /// Optional JSON float array query, e.g. '[0.1, 0.2, 0.3]'.
        #[arg(long, short)]
        query: Option<String>,

        /// Number of nearest neighbours to return (applies to --query).
        #[arg(long, default_value = "5")]
        top_k: usize,
    },

    /// Compare database state between two event counts (semantic diff).
    ///
    /// Replays to --from and --to independently from the same snapshot
    /// baseline and reports the state-hash delta and nearest-neighbour rank
    /// changes for an optional --query vector.
    Diff {
        /// Path to the snapshot file (baseline state).
        #[arg(long)]
        snapshot: String,

        /// Path to the event log file.
        #[arg(long)]
        log: String,

        /// Starting event count (inclusive lower bound).
        #[arg(long)]
        from: u64,

        /// Ending event count (inclusive upper bound).
        #[arg(long)]
        to: u64,

        /// Optional JSON float array for semantic diff, e.g. '[0.1, 0.2]'.
        #[arg(long)]
        query: Option<String>,

        /// Number of nearest neighbours to compare.
        #[arg(long, default_value = "5")]
        top_k: usize,
    },
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Inspect { dir, snapshot, log } => {
            inspect::run(dir, snapshot, log)
        }
        Commands::Verify { snapshot } => {
            verify::run(&snapshot)
        }
        Commands::Timeline { log, limit } => {
            timeline::run(&log, limit)
        }
        Commands::ReplayQuery { snapshot, log, at, query, top_k } => {
            replay_query::run(&snapshot, &log, at, query, top_k)
        }
        Commands::Diff { snapshot, log, from, to, query, top_k } => {
            diff::run(&snapshot, &log, from, to, query, top_k)
        }
    }
}
