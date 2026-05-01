#!/bin/bash
set -e

# Ensure no stale processes are running
pkill -9 valori-node || true
sleep 1

echo "Starting Valori Node for 10k scale test..."
export VALORI_DIM=16
export VALORI_BIND="127.0.0.1:3033"
export VALORI_EVENT_LOG_PATH="test_10k_events.log"

rm -f test_10k_events.log

# Start server
cd /Users/as-mac-0272/Desktop/sass/Valori-Kernel/node
cargo run --release &
SERVER_PID=$!

# Wait for server to start (release build might take longer first time, but we just ran it in dev)
# Actually, I'll use dev build for speed of starting if release is not already built.
# Let's stick to dev for this test environment.
# cargo run &
# SERVER_PID=$!

sleep 15

# Run Scale Test
cd /Users/as-mac-0272/Desktop/sass/Valori-Kernel
export PYTHONPATH=$PYTHONPATH:$(pwd)/python
python3 python/tests/test_scale_10k.py

echo "Killing server..."
kill $SERVER_PID
wait $SERVER_PID || true

echo "Scale Test Complete."
