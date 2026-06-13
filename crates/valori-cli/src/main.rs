// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
use clap::{Parser, Subcommand};
use std::path::PathBuf;
use valori_cli::commands::{cluster, diff, inspect, replay_query, timeline, verify, wizard};

#[derive(Parser)]
#[command(
    name    = "valori",
    version = env!("CARGO_PKG_VERSION"),
    author  = "Varshith Gudur",
    about   = "Valori Forensic CLI — black-box flight recorder for Valori AI memory databases",
    long_about = "Run without arguments (or `valori setup`) to launch the interactive cluster wizard.",
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// Interactive cluster setup and operations wizard (default when no command is given).
    ///
    /// Guides you through architecture choice, node count, and startup, then
    /// drops into a live menu for inserts, search, and membership operations.
    Setup {
        /// IP address to bind API ports to.
        /// Use 127.0.0.1 (default) for local dev; 0.0.0.0 for servers / EC2
        /// so external curl/clients can reach the cluster.
        #[arg(long, default_value = "127.0.0.1")]
        bind: String,
    },

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

    /// Operate a running Raft cluster (status, health, membership).
    ///
    /// Point --url at ANY node's HTTP API. Membership changes are
    /// leader-only; a follower's 403 names the leader to re-point at.
    Cluster {
        #[command(subcommand)]
        action: ClusterAction,
    },
}

#[derive(Subcommand)]
enum ClusterAction {
    /// Leadership, term, log indexes, and the member table.
    Status {
        /// Base URL of any cluster node, e.g. http://10.0.0.1:3000
        #[arg(long, default_value = "http://127.0.0.1:3000")]
        url: String,
    },
    /// Exit 0 when this node sees a leader, exit 1 otherwise.
    Health {
        #[arg(long, default_value = "http://127.0.0.1:3000")]
        url: String,
    },
    /// Join a node: learner catch-up, then voter promotion.
    AddNode {
        /// Base URL of the LEADER's HTTP API.
        #[arg(long, default_value = "http://127.0.0.1:3000")]
        url: String,
        /// The new node's numeric id.
        #[arg(long)]
        id: u64,
        /// The new node's Raft (gRPC) address, host:port.
        #[arg(long)]
        raft_addr: String,
        /// The new node's HTTP API address, host:port.
        #[arg(long, default_value = "")]
        api_addr: String,
    },
    /// Remove a voter (removing the last voter is refused).
    RemoveNode {
        /// Base URL of the LEADER's HTTP API.
        #[arg(long, default_value = "http://127.0.0.1:3000")]
        url: String,
        /// The node id to remove.
        #[arg(long)]
        id: u64,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    match cli.command {
        // No subcommand or explicit `valori setup` → wizard.
        None => wizard::run("127.0.0.1").await,
        Some(Commands::Setup { bind }) => wizard::run(&bind).await,

        Some(Commands::Inspect { dir, snapshot, log }) => inspect::run(dir, snapshot, log),
        Some(Commands::Verify { snapshot }) => verify::run(&snapshot),
        Some(Commands::Timeline { log, limit }) => timeline::run(&log, limit),
        Some(Commands::ReplayQuery { snapshot, log, at, query, top_k }) => {
            replay_query::run(&snapshot, &log, at, query, top_k)
        }
        Some(Commands::Diff { snapshot, log, from, to, query, top_k }) => {
            diff::run(&snapshot, &log, from, to, query, top_k)
        }
        Some(Commands::Cluster { action }) => match action {
            ClusterAction::Status { url } => cluster::status(&url),
            ClusterAction::Health { url } => cluster::health(&url),
            ClusterAction::AddNode { url, id, raft_addr, api_addr } => {
                cluster::add_node(&url, id, &raft_addr, &api_addr)
            }
            ClusterAction::RemoveNode { url, id } => cluster::remove_node(&url, id),
        },
    }
}
