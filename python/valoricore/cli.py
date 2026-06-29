"""
valori CLI — interact with a Valoricore node from the terminal.

Usage:
    valori [--url URL] <command> [options]

Set VALORI_URL to avoid passing --url every time:
    export VALORI_URL=http://localhost:3000
"""

import argparse
import json
import os
import sys
from typing import Optional


def _client(url: str, token: Optional[str] = None):
    from valoricore.remote import SyncRemoteClient
    return SyncRemoteClient(url, auth_token=token)


def _url_from_env(args) -> str:
    url = getattr(args, "url", None) or os.environ.get("VALORI_URL", "http://localhost:3000")
    return url.rstrip("/")


def _token(args) -> Optional[str]:
    return getattr(args, "token", None) or os.environ.get("VALORI_AUTH_TOKEN")


def _print(data):
    if isinstance(data, (dict, list)):
        print(json.dumps(data, indent=2, default=str))
    else:
        print(data)


# ── Commands ──────────────────────────────────────────────────────────────────

def cmd_health(args):
    """Check if the node is reachable and healthy."""
    result = _client(_url_from_env(args), _token(args)).health()
    print(f"status: {result}")


def cmd_version(args):
    """Print the node's software version."""
    result = _client(_url_from_env(args), _token(args)).get_version()
    _print(result)


def cmd_insert(args):
    """Insert a vector (and optional text) into a collection."""
    vector = json.loads(args.vector)
    c = _client(_url_from_env(args), _token(args))
    record_id = c.insert(
        vector,
        text=args.text,
        metadata=json.loads(args.metadata) if args.metadata else None,
        collection=args.collection,
    )
    print(f"record_id: {record_id}")


def cmd_search(args):
    """Search for nearest vectors to a query vector."""
    vector = json.loads(args.vector)
    c = _client(_url_from_env(args), _token(args))
    results = c.search(
        vector,
        k=args.k,
        collection=args.collection,
        query_text=args.query_text,
        decay_half_life_secs=args.decay,
    )
    _print(results)


def cmd_ingest(args):
    """Chunk, embed, and insert a document in one call (requires VALORI_EMBED_PROVIDER on node)."""
    if args.file:
        with open(args.file) as f:
            text = f.read()
    else:
        text = args.text
    if not text:
        print("error: provide --text or --file", file=sys.stderr)
        sys.exit(1)
    c = _client(_url_from_env(args), _token(args))
    result = c.ingest(text, source=args.source, collection=args.collection)
    _print(result)


def cmd_collections(args):
    """List all collections on the node."""
    result = _client(_url_from_env(args), _token(args)).list_collections()
    _print(result)


def cmd_create_collection(args):
    """Create a new collection (namespace)."""
    result = _client(_url_from_env(args), _token(args)).create_collection(args.name)
    _print(result)


def cmd_drop_collection(args):
    """Delete a collection and all its records."""
    c = _client(_url_from_env(args), _token(args))
    c.drop_collection(args.name)
    print(f"dropped: {args.name}")


def cmd_proof(args):
    """Print the BLAKE3 audit receipt for the current node state."""
    result = _client(_url_from_env(args), _token(args)).event_log_proof()
    _print(result)


def cmd_graphrag(args):
    """Run a GraphRAG query — nearest vectors + connected subgraph."""
    vector = json.loads(args.vector)
    result = _client(_url_from_env(args), _token(args)).graphrag(
        vector,
        k=args.k,
        depth=args.depth,
        collection=args.collection,
    )
    _print(result)


def cmd_community_detect(args):
    """Run community detection (label propagation) on the graph."""
    result = _client(_url_from_env(args), _token(args)).community_detect(
        namespace=args.collection,
        max_iter=args.max_iter,
    )
    _print(result)


def cmd_community_overview(args):
    """Show all detected communities sorted by size."""
    result = _client(_url_from_env(args), _token(args)).community_overview()
    _print(result)


def cmd_verify(args):
    """Verify an event log file's BLAKE3 chain offline (no node needed)."""
    from valoricore.verify import verify_log
    result = verify_log(args.log_path, expected_hash=args.expected_hash)
    _print(result)
    if not result.get("ok"):
        sys.exit(1)


def cmd_snapshot_save(args):
    """Trigger the node to write a snapshot to disk."""
    result = _client(_url_from_env(args), _token(args)).save_snapshot(path=args.path)
    _print(result)


def cmd_cluster_status(args):
    """Show Raft cluster status (leader, term, membership)."""
    result = _client(_url_from_env(args), _token(args)).get_cluster_status()
    _print(result)


def cmd_memory_upsert(args):
    """Insert or update an agent memory record."""
    vector = json.loads(args.vector)
    result = _client(_url_from_env(args), _token(args)).memory_upsert(
        vector,
        metadata=json.loads(args.metadata) if args.metadata else None,
    )
    _print(result)


def cmd_memory_search(args):
    """Search agent memory with optional recency decay."""
    vector = json.loads(args.vector)
    result = _client(_url_from_env(args), _token(args)).memory_search(
        vector,
        k=args.k,
        decay_half_life_secs=args.decay,
    )
    _print(result)


# ── Parser ────────────────────────────────────────────────────────────────────

def build_parser() -> argparse.ArgumentParser:
    p = argparse.ArgumentParser(
        prog="valori",
        description="Valoricore CLI — deterministic vector DB with BLAKE3 audit chain.",
        formatter_class=argparse.RawDescriptionHelpFormatter,
        epilog="""
environment variables:
  VALORI_URL          node URL (default: http://localhost:3000)
  VALORI_AUTH_TOKEN   bearer token if auth is enabled on the node

examples:
  valori health
  valori version
  valori collections list
  valori collections create my-project
  valori insert --vector "[0.1, 0.2, 0.3]" --text "hello world"
  valori search --vector "[0.1, 0.2, 0.3]" --k 5
  valori search --vector "[0.1, 0.2, 0.3]" --k 5 --query-text "hello"
  valori ingest --text "long document..." --source paper.pdf
  valori ingest --file /path/to/doc.txt --collection legal
  valori graphrag --vector "[0.1, 0.2, 0.3]" --k 5 --depth 2
  valori community detect
  valori community overview
  valori proof
  valori verify --log-path /var/valori/events.log
  valori snapshot save
  valori cluster status
  valori memory upsert --vector "[0.1, 0.2, 0.3]"
  valori memory search --vector "[0.1, 0.2, 0.3]" --k 5 --decay 86400
""",
    )

    # Global flags
    p.add_argument("--url", metavar="URL", help="node URL (overrides VALORI_URL)")
    p.add_argument("--token", metavar="TOKEN", help="auth bearer token (overrides VALORI_AUTH_TOKEN)")

    sub = p.add_subparsers(dest="command", metavar="command")
    sub.required = True

    # ── health ──
    s = sub.add_parser("health", help="check node is reachable")
    s.set_defaults(func=cmd_health)

    # ── version ──
    s = sub.add_parser("version", help="print node software version")
    s.set_defaults(func=cmd_version)

    # ── proof ──
    s = sub.add_parser("proof", help="print BLAKE3 audit receipt for current state")
    s.set_defaults(func=cmd_proof)

    # ── insert ──
    s = sub.add_parser("insert", help="insert a vector record")
    s.add_argument("--vector", required=True, metavar="JSON", help='e.g. "[0.1, 0.2, 0.3]"')
    s.add_argument("--text", metavar="TEXT", help="raw text to index alongside the vector")
    s.add_argument("--metadata", metavar="JSON", help='e.g. \'{"author": "Alice"}\'')
    s.add_argument("--collection", default="default", metavar="NAME")
    s.set_defaults(func=cmd_insert)

    # ── search ──
    s = sub.add_parser("search", help="find nearest vectors")
    s.add_argument("--vector", required=True, metavar="JSON")
    s.add_argument("--k", type=int, default=5, metavar="N", help="number of results (default: 5)")
    s.add_argument("--collection", default="default", metavar="NAME")
    s.add_argument("--query-text", dest="query_text", metavar="TEXT", help="activate Valori Reranker")
    s.add_argument("--decay", type=float, metavar="SECS", help="half-life in seconds for recency decay")
    s.set_defaults(func=cmd_search)

    # ── ingest ──
    s = sub.add_parser("ingest", help="chunk + embed + insert a document (node must have embed provider)")
    g = s.add_mutually_exclusive_group()
    g.add_argument("--text", metavar="TEXT")
    g.add_argument("--file", metavar="PATH")
    s.add_argument("--source", metavar="NAME", help="label for the source document")
    s.add_argument("--collection", default="default", metavar="NAME")
    s.set_defaults(func=cmd_ingest)

    # ── graphrag ──
    s = sub.add_parser("graphrag", help="vector search + connected subgraph")
    s.add_argument("--vector", required=True, metavar="JSON")
    s.add_argument("--k", type=int, default=5, metavar="N")
    s.add_argument("--depth", type=int, default=2, metavar="N", help="graph traversal depth (default: 2)")
    s.add_argument("--collection", default="default", metavar="NAME")
    s.set_defaults(func=cmd_graphrag)

    # ── collections ──
    sc = sub.add_parser("collections", help="manage collections")
    sc_sub = sc.add_subparsers(dest="collections_cmd", metavar="subcommand")
    sc_sub.required = True

    s = sc_sub.add_parser("list", help="list all collections")
    s.set_defaults(func=cmd_collections)

    s = sc_sub.add_parser("create", help="create a collection")
    s.add_argument("name")
    s.set_defaults(func=cmd_create_collection)

    s = sc_sub.add_parser("drop", help="delete a collection and all records")
    s.add_argument("name")
    s.set_defaults(func=cmd_drop_collection)

    # ── community ──
    cc = sub.add_parser("community", help="community detection and overview")
    cc_sub = cc.add_subparsers(dest="community_cmd", metavar="subcommand")
    cc_sub.required = True

    s = cc_sub.add_parser("detect", help="run label propagation on the graph")
    s.add_argument("--collection", default="default", metavar="NAME")
    s.add_argument("--max-iter", dest="max_iter", type=int, default=10, metavar="N")
    s.set_defaults(func=cmd_community_detect)

    s = cc_sub.add_parser("overview", help="show all communities sorted by size")
    s.set_defaults(func=cmd_community_overview)

    # ── verify ──
    s = sub.add_parser("verify", help="verify an event log BLAKE3 chain offline")
    s.add_argument("--log-path", dest="log_path", required=True, metavar="PATH")
    s.add_argument("--expected-hash", dest="expected_hash", metavar="HEX", help="expected final hash")
    s.set_defaults(func=cmd_verify)

    # ── snapshot ──
    sc = sub.add_parser("snapshot", help="snapshot management")
    sc_sub = sc.add_subparsers(dest="snapshot_cmd", metavar="subcommand")
    sc_sub.required = True

    s = sc_sub.add_parser("save", help="trigger node to write a snapshot")
    s.add_argument("--path", metavar="PATH", help="override snapshot path on the node")
    s.set_defaults(func=cmd_snapshot_save)

    # ── cluster ──
    sc = sub.add_parser("cluster", help="cluster management")
    sc_sub = sc.add_subparsers(dest="cluster_cmd", metavar="subcommand")
    sc_sub.required = True

    s = sc_sub.add_parser("status", help="show Raft leader, term, membership")
    s.set_defaults(func=cmd_cluster_status)

    # ── memory ──
    sc = sub.add_parser("memory", help="agent memory primitives")
    sc_sub = sc.add_subparsers(dest="memory_cmd", metavar="subcommand")
    sc_sub.required = True

    s = sc_sub.add_parser("upsert", help="insert or update an agent memory")
    s.add_argument("--vector", required=True, metavar="JSON")
    s.add_argument("--metadata", metavar="JSON")
    s.set_defaults(func=cmd_memory_upsert)

    s = sc_sub.add_parser("search", help="search agent memory")
    s.add_argument("--vector", required=True, metavar="JSON")
    s.add_argument("--k", type=int, default=5, metavar="N")
    s.add_argument("--decay", type=float, metavar="SECS", help="half-life in seconds")
    s.set_defaults(func=cmd_memory_search)

    return p


def main():
    parser = build_parser()
    args = parser.parse_args()

    try:
        args.func(args)
    except KeyboardInterrupt:
        sys.exit(130)
    except Exception as e:
        print(f"error: {e}", file=sys.stderr)
        sys.exit(1)


if __name__ == "__main__":
    main()
