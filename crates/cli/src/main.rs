use clap::{Parser, Subcommand};
use std::path::PathBuf;
use valori_cli::commands::{diff, inspect, replay_query, timeline, verify};

#[derive(Parser)]
#[command(name = "valori")]
#[command(about = "Valori Forensic CLI - The Black Box Flight Recorder for AI Memory", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Inspect the database files and show status.
    /// If --dir is provided, it tries to auto-resolve snapshot.val, events.log, and metadata.idx.
    Inspect {
        /// Optional directory containing the Valori files.
        #[arg(long, short)]
        dir: Option<PathBuf>,

        /// Path to the snapshot file (overrides auto-detection)
        #[arg(long)]
        snapshot_path: Option<String>,

        /// Path to the WAL file (overrides auto-detection)
        #[arg(long)]
        wal_path: Option<String>,

        /// Path to the Index file (overrides auto-detection)
        #[arg(long)]
        idx_path: Option<String>,
    },
    /// Verify the integrity of a snapshot file
    Verify {
        snapshot_path: String,
    },
    /// List the event timeline
    Timeline {
        idx_path: String,
    },
    /// Fast-forward replay to a specific point and simulate a query
    ReplayQuery {
        snapshot_path: String,
        wal_path: String,
        
        /// The target event ID to time travel to
        #[arg(long, short)]
        at: u64,

        /// Optional JSON query to simulate
        #[arg(long, short)]
        query: Option<String>,
    },
    /// Compare system state at two points in time
    Diff {
        snapshot_path: String,
        wal_path: String,
        
        /// From Event ID
        #[arg(long)]
        from: u64,

        /// To Event ID
        #[arg(long)]
        to: u64,

        /// Optional Query for Semantic Diff
        #[arg(long)]
        query: Option<String>,
    },
}

fn main() -> anyhow::Result<()> {
    println!(r#"
__     __    _            _ 
\ \   / /_ _| | ___  _ __(_)
 \ \ / / _` | |/ _ \| '__| |
  \ V / (_| | | (_) | |  | |
   \_/ \__,_|_|\___/|_|  |_|
   
   Valori Forensic Tool v0.1.0-mvp
   "Flight Recorder" Build
    "#);

    let cli = Cli::parse();

    match cli.command {
        Commands::Inspect {
            dir,
            snapshot_path,
            wal_path,
            idx_path,
        } => inspect::run(dir, snapshot_path, wal_path, idx_path),
        Commands::Verify { snapshot_path } => verify::run(&snapshot_path),
        Commands::Timeline { idx_path } => timeline::run(&idx_path),
        Commands::ReplayQuery {
            snapshot_path,
            wal_path,
            at,
            query,
        } => replay_query::run(&snapshot_path, &wal_path, at, query),
        Commands::Diff {
            snapshot_path,
            wal_path,
            from,
            to,
            query,
        } => diff::run(&snapshot_path, &wal_path, from, to, query),
    }
}
