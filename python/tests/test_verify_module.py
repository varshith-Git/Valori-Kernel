# Copyright (c) 2025 Varshith Gudur. Licensed under MIT OR Apache-2.0.
"""
Tests for valoricore.verify — AnchorVerifier, VerifyReport, verify_log.

These tests run without a live Valori node or the Rust binaries:
- AnchorVerifier: uses a real Ed25519 keypair generated in-test
- VerifyReport: parses synthetic report dicts
- verify_log: subprocess path tested via mock; binary-not-found path tested live
"""

import asyncio
import json
import struct
import tempfile
from pathlib import Path
from unittest.mock import MagicMock, patch

import pytest

from valoricore.exceptions import IntegrityError, TamperDetected
from valoricore.verify import AnchorVerifier, TamperFinding, VerifyReport, verify_log

# ── helpers ───────────────────────────────────────────────────────────────────

_DOMAIN_SEP = b"valori-anchor-v1\x00"


def _make_key():
    """Return a cryptography Ed25519PrivateKey (same library the verifier uses)."""
    try:
        from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PrivateKey
        return Ed25519PrivateKey.generate()
    except ImportError:
        pytest.skip("cryptography not installed; install valoricore[verify]")


def _make_anchor_dict(chain_head, event_count, state_hash, anchored_at_unix,
                      private_key, note=None):
    """Build a valid anchor dict signed with a cryptography Ed25519PrivateKey."""
    from cryptography.hazmat.primitives.serialization import Encoding, PublicFormat

    msg = (
        _DOMAIN_SEP
        + chain_head
        + struct.pack("<Q", event_count)
        + state_hash
        + struct.pack("<Q", anchored_at_unix)
    )
    sig_bytes = private_key.sign(msg)           # raw 64 bytes
    pub_bytes = private_key.public_key().public_bytes(Encoding.Raw, PublicFormat.Raw)
    d = {
        "schema_version": 1,
        "chain_head": chain_head.hex(),
        "event_count": event_count,
        "state_hash": state_hash.hex(),
        "anchored_at": "2026-06-10T14:02:11Z",
        "anchored_at_unix": anchored_at_unix,
        "public_key_ed25519": pub_bytes.hex(),
        "signature_ed25519": sig_bytes.hex(),
    }
    if note:
        d["note"] = note
    return d


# ── AnchorVerifier — round-trip ───────────────────────────────────────────────

class TestAnchorVerifierSignature:
    def test_valid_signature_passes(self):
        sk = _make_key()
        chain_head = bytes(range(32))
        state_hash = bytes(reversed(range(32)))
        d = _make_anchor_dict(chain_head, 2007, state_hash, 1749561731, sk)
        # Should not raise
        anchor = AnchorVerifier.from_dict(d, verify=True)
        assert anchor.event_count == 2007
        assert anchor.chain_head == chain_head
        assert anchor.state_hash == state_hash

    def test_corrupted_signature_raises_integrity_error(self):
        sk = _make_key()
        d = _make_anchor_dict(bytes(32), 100, bytes(32), 1749561731, sk)
        # Corrupt one byte of the signature
        sig_hex = list(d["signature_ed25519"])
        sig_hex[0] = "f" if sig_hex[0] != "f" else "0"
        d["signature_ed25519"] = "".join(sig_hex)
        with pytest.raises(IntegrityError):
            AnchorVerifier.from_dict(d, verify=True)

    def test_altered_event_count_breaks_message(self):
        sk = _make_key()
        d = _make_anchor_dict(bytes(32), 100, bytes(32), 1749561731, sk)
        d["event_count"] = 101  # change after signing
        with pytest.raises(IntegrityError):
            AnchorVerifier.from_dict(d, verify=True)

    def test_note_roundtrip(self):
        sk = _make_key()
        d = _make_anchor_dict(bytes(32), 5, bytes(32), 0, sk, note="audit 2026-Q2")
        anchor = AnchorVerifier.from_dict(d, verify=True)
        assert anchor.note == "audit 2026-Q2"

    def test_load_from_file(self, tmp_path):
        sk = _make_key()
        d = _make_anchor_dict(bytes(32), 1, bytes(32), 1749561731, sk)
        p = tmp_path / "events.anchor"
        p.write_text(json.dumps(d))
        anchor = AnchorVerifier.load(p)
        assert anchor.event_count == 1


class TestAnchorVerifierCheckAgainstNode:
    def test_matching_hash_does_not_raise(self):
        sk = _make_key()
        state_hash = bytes(range(32))
        d = _make_anchor_dict(bytes(32), 10, state_hash, 0, sk)
        anchor = AnchorVerifier.from_dict(d, verify=True)

        client = MagicMock()
        client.get_state_hash.return_value = state_hash.hex()
        anchor.check_against_node(client, verify_signature=False)

    def test_mismatched_hash_raises_tamper_detected(self):
        sk = _make_key()
        state_hash = bytes(range(32))
        d = _make_anchor_dict(bytes(32), 10, state_hash, 0, sk)
        anchor = AnchorVerifier.from_dict(d, verify=True)

        client = MagicMock()
        client.get_state_hash.return_value = "aa" * 32  # wrong hash
        with pytest.raises(TamperDetected):
            anchor.check_against_node(client, verify_signature=False)

    def test_async_matching_does_not_raise(self):
        sk = _make_key()
        state_hash = bytes(range(32))
        d = _make_anchor_dict(bytes(32), 10, state_hash, 0, sk)
        anchor = AnchorVerifier.from_dict(d, verify=True)

        async def async_hash():
            return state_hash.hex()

        class AsyncClient:
            get_state_hash = staticmethod(async_hash)

        asyncio.run(anchor.async_check_against_node(AsyncClient(), verify_signature=False))


# ── VerifyReport ──────────────────────────────────────────────────────────────

_VERIFIED_REPORT = {
    "schema_version": 1,
    "verdict": "verified",
    "log": {"path": "/tmp/events.log", "size_bytes": 121993, "format_version": 2, "dim": 4},
    "replay": {
        "events_replayed": 2007,
        "checkpoints_seen": 0,
        "state_hash": "76b8bf40571573eaca231a122d1ad3db5e3e03ebc48fa493476f0b7da727bd2d",
        "chain_head": "5569f61ff7885bf25630ef8149d92a350bf49dccf9e9aac2802dff4cf69b1f0b",
    },
    "expected_hash": "76b8bf40571573eaca231a122d1ad3db5e3e03ebc48fa493476f0b7da727bd2d",
    "generated_at": "2026-06-10T14:02:11Z",
    "generated_at_unix": 1749561731,
    "finding": None,
}

_CHAIN_REPORT = {
    **_VERIFIED_REPORT,
    "verdict": "tampered_chain",
    "finding": {
        "type": "chain_breach",
        "breach_entry_no": 1007,
        "breach_byte_offset": 60905,
        "likely_altered_entry_no": 1006,
        "likely_altered_entry_payload": "InsertRecord { id: RecordId(1005), ... }",
        "breach_entry_committed": "2025-06-15T15:23:26Z",
        "breach_entry_committed_unix": 1750001006,
        "computed_chain_head": "aa" * 32,
        "stored_prev_hash": "bb" * 32,
        "events_clean_before_breach": 1006,
    },
}

_STRUCTURAL_REPORT = {
    **_VERIFIED_REPORT,
    "verdict": "tampered_structural",
    "finding": {
        "type": "structural",
        "failed_entry_no": 5,
        "failed_byte_offset": 300,
        "trailing_unreadable_bytes": 9999,
        "events_clean_before_failure": 4,
    },
}


class TestVerifyReport:
    def test_verified_report(self):
        r = VerifyReport.from_dict(_VERIFIED_REPORT)
        assert r.is_verified
        assert not r.is_tampered
        assert r.events_replayed == 2007
        assert r.finding is None

    def test_chain_breach_report(self):
        r = VerifyReport.from_dict(_CHAIN_REPORT)
        assert not r.is_verified
        assert r.is_tampered
        assert r.finding is not None
        assert r.finding.type == "chain_breach"
        assert r.finding.breach_entry_no == 1007
        assert r.finding.likely_altered_entry_no == 1006
        assert "entry #1007" in r.finding.summary()

    def test_structural_report(self):
        r = VerifyReport.from_dict(_STRUCTURAL_REPORT)
        assert r.finding.type == "structural"
        assert "entry #5" in r.finding.summary()

    def test_raise_if_tampered_raises(self):
        r = VerifyReport.from_dict(_CHAIN_REPORT)
        with pytest.raises(TamperDetected, match="entry #1007"):
            r.raise_if_tampered()

    def test_raise_if_tampered_noop_when_verified(self):
        r = VerifyReport.from_dict(_VERIFIED_REPORT)
        r.raise_if_tampered()  # must not raise

    def test_from_file(self, tmp_path):
        p = tmp_path / "report.json"
        p.write_text(json.dumps(_VERIFIED_REPORT))
        r = VerifyReport.from_file(p)
        assert r.is_verified


# ── verify_log ────────────────────────────────────────────────────────────────

class TestVerifyLog:
    def test_binary_not_found_raises(self, tmp_path):
        dummy_log = tmp_path / "events.log"
        dummy_log.write_bytes(b"\x00" * 16)
        with pytest.raises(FileNotFoundError, match="valori-verify"):
            verify_log(dummy_log, binary="/nonexistent/valori-verify")

    def test_subprocess_called_with_correct_args(self, tmp_path):
        dummy_log = tmp_path / "events.log"
        dummy_log.write_bytes(b"\x00" * 16)
        report_data = json.dumps(_VERIFIED_REPORT)

        def fake_run(cmd, **_kwargs):
            # Write the report to the --report path in cmd
            idx = cmd.index("--report")
            Path(cmd[idx + 1]).write_text(report_data)

        with patch("subprocess.run", side_effect=fake_run):
            r = verify_log(dummy_log, expected_hash="ab" * 32)

        assert r.is_verified

    def test_raise_on_tamper_propagates(self, tmp_path):
        dummy_log = tmp_path / "events.log"
        dummy_log.write_bytes(b"\x00" * 16)
        report_data = json.dumps(_CHAIN_REPORT)

        def fake_run(cmd, **_kwargs):
            idx = cmd.index("--report")
            Path(cmd[idx + 1]).write_text(report_data)

        with patch("subprocess.run", side_effect=fake_run):
            with pytest.raises(TamperDetected):
                verify_log(dummy_log, raise_on_tamper=True)

    def test_report_written_to_caller_path(self, tmp_path):
        dummy_log = tmp_path / "events.log"
        dummy_log.write_bytes(b"\x00" * 16)
        report_path = tmp_path / "findings.json"
        report_data = json.dumps(_VERIFIED_REPORT)

        def fake_run(cmd, **_kwargs):
            idx = cmd.index("--report")
            Path(cmd[idx + 1]).write_text(report_data)

        with patch("subprocess.run", side_effect=fake_run):
            verify_log(dummy_log, report_path=report_path)

        # File must still exist since caller supplied the path
        assert report_path.exists()
