#!/bin/bash
cargo run -p valori-node > node.log 2>&1 &
SERVER_PID=$!
sleep 3
PYTHONUNBUFFERED=1 PYTHONPATH=python .venv/bin/python test_remote_graph.py
EXIT_CODE=$?
kill $SERVER_PID
exit $EXIT_CODE
