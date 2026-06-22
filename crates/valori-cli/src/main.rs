// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
use clap::{Parser, Subcommand};
use std::path::PathBuf;
use valori_cli::commands::{cluster, diff, import, inspect, replay_query, timeline, verify, wizard};

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

    /// Import vectors from an external source into a running Valori node.
    ///
    /// Validates that the source dimension matches the target node's VALORI_DIM
    /// before touching any data. Supports resumable imports via a sidecar file.
    Import {
        #[command(subcommand)]
        source: ImportSource,
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
    /// Guided rolling upgrade — drain, upgrade, rejoin each node in turn.
    ///
    /// Point --url at any cluster node. The command prints a step-by-step plan,
    /// then walks through each node interactively: it tells you which node to
    /// stop, waits for you to upgrade the binary and restart it, then polls
    /// until the node is healthy before moving to the next one. The current
    /// leader is always upgraded last to minimise write disruption.
    Upgrade {
        /// Base URL of any cluster node, e.g. http://10.0.0.1:3000
        #[arg(long, default_value = "http://127.0.0.1:3000")]
        url: String,
        /// The target version string shown in the step instructions, e.g. 0.3.0
        #[arg(long)]
        target_version: String,
    },
}

#[derive(Subcommand)]
enum ImportSource {
    /// Import from a Qdrant collection via the scroll API.
    ///
    /// Example:
    ///   valori import qdrant \
    ///     --url http://localhost:6333 \
    ///     --collection my-vectors \
    ///     --target-url http://localhost:3000 \
    ///     --target-collection my-vectors
    Qdrant {
        /// Base URL of the Qdrant HTTP API, e.g. http://localhost:6333
        #[arg(long, default_value = "http://localhost:6333")]
        url: String,

        /// Source collection name in Qdrant.
        #[arg(long)]
        collection: String,

        /// Base URL of the target Valori node, e.g. http://localhost:3000
        #[arg(long, default_value = "http://localhost:3000")]
        target_url: String,

        /// Target collection name in Valori (created if it doesn't exist).
        #[arg(long, default_value = "default")]
        target_collection: String,

        /// Number of records per scroll page.
        #[arg(long, default_value = "100")]
        batch_size: usize,

        /// Resume from a previous interrupted import (reads sidecar file).
        #[arg(long, default_value_t = false)]
        resume: bool,

        /// Bearer token for Valori authentication (VALORI_AUTH_TOKEN or an API key).
        #[arg(long)]
        token: Option<String>,
    },

    /// Import from a JSONL file.
    ///
    /// Each line must be a JSON object with a "vector" field (array of floats).
    /// Optional fields: "metadata" (string), "tag" (u64).
    /// Aliases accepted for "vector": "embedding", "values".
    /// Aliases accepted for "metadata": "text", "content", "payload".
    ///
    /// Example line:
    ///   {"vector": [0.1, 0.2, 0.3], "metadata": "Hello, world", "tag": 0}
    Jsonl {
        /// Path to the JSONL file to import.
        file: PathBuf,

        /// Base URL of the target Valori node.
        #[arg(long, default_value = "http://localhost:3000")]
        target_url: String,

        /// Target collection name in Valori (created if it doesn't exist).
        #[arg(long, default_value = "default")]
        target_collection: String,

        /// Records to buffer before flushing to Valori (controls memory usage).
        #[arg(long, default_value = "100")]
        batch_size: usize,

        /// Bearer token for Valori authentication.
        #[arg(long)]
        token: Option<String>,
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
            ClusterAction::Upgrade { url, target_version } => {
                cluster::upgrade(&url, &target_version)
            }
        },

        Some(Commands::Import { source }) => match source {
            ImportSource::Qdrant {
                url,
                collection,
                target_url,
                target_collection,
                batch_size,
                resume,
                token,
            } => import::run_qdrant(import::QdrantImportArgs {
                qdrant_url: url,
                source_collection: collection,
                target_url,
                target_collection,
                batch_size,
                resume,
                token,
            }),
            ImportSource::Jsonl {
                file,
                target_url,
                target_collection,
                batch_size,
                token,
            } => import::run_jsonl(import::JsonlImportArgs {
                file,
                target_url,
                target_collection,
                batch_size,
                token,
            }),
        },
    }
}
