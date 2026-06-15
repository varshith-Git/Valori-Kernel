#!/usr/bin/env bash
# Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
#
# Starts a 3-node Valori cluster locally without Docker.
# Useful for local development and debugging when Docker is not available.

set -e

echo "=========================================================="
echo " Starting Local 3-Node Valori Cluster (No Docker)"
echo "=========================================================="

# Build the binary first
echo "Building valori-node..."
cargo build --release -p valori-node

# Create isolated data directories for the 3 nodes
mkdir -p .data/node-1 .data/node-2 .data/node-3

# Shared Cluster Topology
# Format: id=raft_addr/api_addr,...
export VALORI_CLUSTER_MEMBERS="1=127.0.0.1:3101/127.0.0.1:3001,2=127.0.0.1:3102/127.0.0.1:3002,3=127.0.0.1:3103/127.0.0.1:3003"

# Setup trap to gracefully kill all background nodes on Ctrl+C
trap 'echo "\nStopping cluster..."; kill $(jobs -p) 2>/dev/null; exit' INT TERM EXIT

echo "\nStarting Node 1 (Leader/Init)..."
VALORI_NODE_ID=1 \
VALORI_CLUSTER_INIT=1 \
VALORI_BIND="127.0.0.1:3001" \
VALORI_RAFT_BIND="127.0.0.1:3101" \
VALORI_EVENT_LOG_PATH=".data/node-1/events.log" \
VALORI_SNAPSHOT_PATH=".data/node-1/state.snap" \
VALORI_RAFT_LOG_PATH=".data/node-1/raft.redb" \
./target/release/valori-node > .data/node-1.log 2>&1 &

sleep 1 # Give Node 1 a tiny head start to initialize the cluster

echo "Starting Node 2 (Follower)..."
VALORI_NODE_ID=2 \
VALORI_BIND="127.0.0.1:3002" \
VALORI_RAFT_BIND="127.0.0.1:3102" \
VALORI_EVENT_LOG_PATH=".data/node-2/events.log" \
VALORI_SNAPSHOT_PATH=".data/node-2/state.snap" \
VALORI_RAFT_LOG_PATH=".data/node-2/raft.redb" \
./target/release/valori-node > .data/node-2.log 2>&1 &

echo "Starting Node 3 (Follower)..."
VALORI_NODE_ID=3 \
VALORI_BIND="127.0.0.1:3003" \
VALORI_RAFT_BIND="127.0.0.1:3103" \
VALORI_EVENT_LOG_PATH=".data/node-3/events.log" \
VALORI_SNAPSHOT_PATH=".data/node-3/state.snap" \
VALORI_RAFT_LOG_PATH=".data/node-3/raft.redb" \
./target/release/valori-node > .data/node-3.log 2>&1 &

echo "\nCluster is running in the background!"
echo "----------------------------------------------------------"
echo "API Ports:  Node 1: 3001 | Node 2: 3002 | Node 3: 3003"
echo "Raft Ports: Node 1: 3101 | Node 2: 3102 | Node 3: 3103"
echo "----------------------------------------------------------"
echo "Logs are being written to:"
echo "  - .data/node-1.log"
echo "  - .data/node-2.log"
echo "  - .data/node-3.log"
echo "\nTo stop the cluster, just press Ctrl+C"

# Wait for all background jobs to keep the script running
wait
