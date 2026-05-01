#!/bin/bash
set -e

echo "Starting Valori Node on port 3032..."
export VALORI_DIM=8
export VALORI_BIND="127.0.0.1:3032"
export VALORI_EVENT_LOG_PATH="test_python_events.log"

rm -f test_python_events.log

# Start server
cd node
cargo run &
SERVER_PID=$!

# Wait for server to start
sleep 15

# Run Python test
cd ..
python3 test_python_remote.py

echo "Killing server..."
kill $SERVER_PID
wait $SERVER_PID || true

echo "Python Test Complete."
