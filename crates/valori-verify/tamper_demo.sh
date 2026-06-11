#!/usr/bin/env bash
# Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
#
# tamper_demo.sh — the 30-second pitch, in executable form.
#
#   1. Generate a real event log (v2 wire format, hash chain) and its state hash.
#   2. Verify it offline                    → VERIFIED, chain intact
#   3a. Flip ONE byte deep in a vector.     → TAMPERED (chain breach at entry #N)
#       The verifier names the exact event, its payload, and the commit timestamp.
#   3b. Corrupt the log structure.          → TAMPERED (structural), entry + offset.
#
# No server. No network. Just math.
#
# Usage:  ./verify/tamper_demo.sh [event_count]   (default 2000)

set -euo pipefail

COUNT="${1:-2000}"
DEMO_DIR="$(mktemp -d /tmp/valori_tamper_demo.XXXXXX)"
LOG="$DEMO_DIR/events.log"
trap 'rm -rf "$DEMO_DIR"' EXIT

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
BIN="$ROOT/target/release"

echo "── building verifier ──────────────────────────────────────────"
cargo build -p valori-verify --release --quiet
echo

echo "── 1. generate a ${COUNT}-event log (v2, hash chain) ──────────"
HASH="$("$BIN/make-demo-log" "$LOG" "$COUNT")"
echo "    expected state hash: $HASH"
echo

echo "── 2. verify the pristine log ─────────────────────────────────"
"$BIN/valori-verify" "$LOG" --expected-hash "$HASH" --report "$DEMO_DIR/pristine.json"
echo
echo "    forensic report: $DEMO_DIR/pristine.json"
echo

# Flip one byte at OFFSET in FILE (XOR 0xFF — guaranteed to change).
flip_byte() {
    local file="$1" offset="$2"
    local original flipped
    original=$(od -An -tu1 -j "$offset" -N 1 "$file" | tr -d ' ')
    flipped=$((original ^ 0xFF))
    echo "    byte at offset $offset: $original → $flipped"
    printf "$(printf '\\x%02x' "$flipped")" \
        | dd of="$file" bs=1 seek="$offset" conv=notrunc status=none
}

# Run the verifier expecting failure; abort the demo if it (wrongly) passes.
expect_tampered() {
    local file="$1" report="$2"
    local status=0
    "$BIN/valori-verify" "$file" --expected-hash "$HASH" --report "$report" || status=$?
    if [ "$status" -eq 0 ]; then
        echo "FATAL: tampering was NOT detected — this should never happen" >&2
        exit 99
    fi
    echo
    echo "    forensic report: $report"
    echo "    (verifier exit code: $status)"
}

SIZE=$(wc -c < "$LOG" | tr -d ' ')
cp "$LOG" "$LOG.content_attack"
cp "$LOG" "$LOG.structure_attack"

echo "── 3a. attack 1: alter a stored vector value ──────────────────"
# Flip a byte deep inside a vector payload (past the 16-byte header, well
# into the entry bodies).  The entry still decodes cleanly — the chain
# catches it and reports the exact event number and commit timestamp.
flip_byte "$LOG.content_attack" $((SIZE / 2))
echo
expect_tampered "$LOG.content_attack" "$DEMO_DIR/content_attack.json"
echo

echo "── 3b. attack 2: corrupt the log structure ────────────────────"
flip_byte "$LOG.structure_attack" 20
echo
expect_tampered "$LOG.structure_attack" "$DEMO_DIR/structure_attack.json"
echo

echo "── done: both attacks caught, pristine log verifies ───────────"
