#!/bin/bash
set -e

echo "Starting Valori Node in background..."
export VALORI_DIM=4
export VALORI_BIND="127.0.0.1:3031"
export VALORI_EVENT_LOG_PATH="test_events.log"

# Clean up old state
rm -f test_events.log

# Start server
cd node
cargo run &
SERVER_PID=$!

# Wait for server to start
sleep 15

echo "Inserting vectors..."
# Insert Vector 1 [1.0, 2.0, 3.0, 4.0]
curl -X POST http://127.0.0.1:3031/records -H "Content-Type: application/json" -d '{"values": [1.0, 2.0, 3.0, 4.0]}'
echo ""

# Insert Vector 2 [10.0, 20.0, 30.0, 40.0]
curl -X POST http://127.0.0.1:3031/records -H "Content-Type: application/json" -d '{"values": [10.0, 20.0, 30.0, 40.0]}'
echo ""

echo "Searching for [1.1, 2.1, 3.1, 4.1] (should find ID 0)..."
curl -X POST http://127.0.0.1:3031/search -H "Content-Type: application/json" -d '{"query": [1.1, 2.1, 3.1, 4.1], "k": 2}'
echo ""

echo "Searching for [9.9, 19.9, 29.9, 39.9] (should find ID 1)..."
curl -X POST http://127.0.0.1:3031/search -H "Content-Type: application/json" -d '{"query": [9.9, 19.9, 29.9, 39.9], "k": 2}'
echo ""

echo "Getting Proof..."
curl -X GET http://127.0.0.1:3031/proof
echo ""

echo "Killing server..."
kill $SERVER_PID
wait $SERVER_PID || true

echo "Test complete."
