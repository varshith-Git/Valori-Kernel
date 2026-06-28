"""
Valori tamper detection demo
=============================

    pip install valoricore
    python examples/tamper_demo.py

Shows the core trust guarantee in four steps:
  1. Write three memories, record the BLAKE3 state hash.
  2. Flip one byte in the on-disk event log — simulating a silent corruption
     or malicious edit.
  3. Reload from the corrupted log.
  4. Watch the state hash diverge and the chain break.

No server, no API key, no Rust toolchain.

To also run the full chain replay with exact byte-offset detection:
  cargo build -p valori-verify --release
  # tamper_demo.py will auto-detect the binary and run it.
"""

import math
import os
import shutil
import struct
import subprocess
import sys

from valoricore import MemoryClient

DB     = "./tamper_demo_db"
LOG    = f"{DB}/events.log"
DIM    = 16

def embed(text: str) -> list:
    s = sum(ord(c) for c in text)
    return [math.sin(s + i * 0.3) for i in range(DIM)]

def separator(title: str) -> None:
    print(f"\n{'─' * 58}")
    print(f"  {title}")
    print('─' * 58)

# ── Step 1: write records ─────────────────────────────────────────────────────
if os.path.exists(DB):
    shutil.rmtree(DB)

separator("Step 1 — write three memories")
db = MemoryClient(path=DB, dim=DIM)
db.add_document(text="Valori proves it never lost your data.", embed=embed)
db.add_document(text="Fixed-point math is bit-identical on every machine.", embed=embed)
db.add_document(text="Every mutation is hash-chained in the event log.", embed=embed)

good_hash = db.get_state_hash()
print(f"  records written: 3")
print(f"  state hash : {good_hash}")
print(f"  events.log : {os.path.getsize(LOG)} bytes")

# ── Step 2: corrupt one byte in the event log ─────────────────────────────────
separator("Step 2 — flip byte 64 in events.log (simulating silent corruption)")
with open(LOG, "r+b") as f:
    f.seek(64)
    original_byte = f.read(1)[0]
    f.seek(64)
    f.write(bytes([original_byte ^ 0xFF]))   # flip all bits of that byte
print(f"  byte 64: 0x{original_byte:02x} → 0x{original_byte ^ 0xFF:02x}")

# ── Step 3: reload from corrupted log ────────────────────────────────────────
separator("Step 3 — reload from the corrupted log")
db2 = MemoryClient(path=DB, dim=DIM)
corrupt_hash = db2.get_state_hash()
print(f"  state hash : {corrupt_hash}")

# ── Step 4: verdict ───────────────────────────────────────────────────────────
separator("Step 4 — verdict")
if good_hash != corrupt_hash:
    print("  ✗ TAMPER DETECTED")
    print(f"    expected : {good_hash}")
    print(f"    replayed : {corrupt_hash}")
    print()
    print("  One flipped bit → completely different hash.")
    print("  An attacker cannot make the hashes agree without")
    print("  breaking BLAKE3 (no known pre-image attack).")
else:
    # This branch should never execute.
    print("  Hash matched despite corruption — please report this as a bug.")

# ── Optional: deep chain replay with valori-verify ────────────────────────────
verify_bin = shutil.which("valori-verify") or \
             "target/release/valori-verify" if os.path.exists("target/release/valori-verify") else None

if verify_bin and os.path.exists(verify_bin):
    separator("Bonus — valori-verify chain replay (exact byte offset)")
    result = subprocess.run(
        [verify_bin, LOG],
        capture_output=True, text=True
    )
    output = (result.stdout + result.stderr).strip()
    for line in output.splitlines()[:15]:
        print(f"  {line}")
else:
    print()
    print("  To see exact byte-offset tamper detection:")
    print("    cargo build -p valori-verify --release")
    print("    python examples/tamper_demo.py")

# ── Cleanup ───────────────────────────────────────────────────────────────────
shutil.rmtree(DB)
print()
