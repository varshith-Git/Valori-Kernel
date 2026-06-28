# Copyright (c) 2025 Varshith Gudur. Licensed under MIT OR Apache-2.0.
"""
valoricore.verify — integrity verification helpers
===================================================

Three entry points, one import:

    from valoricore.verify import AnchorVerifier, VerifyReport, verify_log

``AnchorVerifier``
    Parse an ``.anchor`` file produced by ``valori-anchor create``, verify its
    Ed25519 signature, and optionally compare against a live node's state hash.
    Pure Python — no subprocess, no log file.  Requires ``cryptography``.

``VerifyReport``
    Structured dataclass that mirrors the JSON schema of ``valori-verify
    --report``.  Use it to drive CI alerts, compliance dashboards, or Slack
    notifications based on tamper findings.

``verify_log(log_path, ...)``
    Thin subprocess wrapper: spawns ``valori-verify``, captures the
    ``--report`` JSON, returns a ``VerifyReport``.  The heavy chain-replay
    logic stays in the auditable Rust binary.

Requires ``pip install valoricore[verify]`` (adds ``cryptography``).
The ``verify_log`` function also needs the ``valori-verify`` binary on PATH
(build with ``cargo build -p valori-verify --release``).
"""

from __future__ import annotations

import json
import os
import struct
import subprocess
import tempfile
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any, Dict, Optional, Union

from .exceptions import IntegrityError, TamperDetected

__all__ = [
    "AnchorVerifier",
    "TamperFinding",
    "VerifyReport",
    "verify_log",
]

# ── Ed25519 import (optional dep) ─────────────────────────────────────────────

def _ed25519_verify(public_key_bytes: bytes, signature: bytes, message: bytes) -> None:
    """Verify an Ed25519 signature, raising IntegrityError on failure."""
    try:
        from cryptography.exceptions import InvalidSignature
        from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PublicKey
    except ImportError as exc:
        raise ImportError(
            "Ed25519 verification requires the 'cryptography' library.\n"
            "Install it with:  pip install 'valoricore[verify]'\n"
            "or:               pip install cryptography"
        ) from exc

    key = Ed25519PublicKey.from_public_bytes(public_key_bytes)
    try:
        key.verify(signature, message)
    except InvalidSignature:
        raise IntegrityError(
            "Ed25519 signature verification failed — the anchor has been tampered with. "
            f"Public key: {public_key_bytes.hex()}"
        )


# ── AnchorVerifier ────────────────────────────────────────────────────────────

_ANCHOR_DOMAIN_SEP = b"valori-anchor-v1\x00"  # 17 bytes


@dataclass(frozen=True)
class AnchorVerifier:
    """
    Parsed and signature-verified representation of a ``.anchor`` file.

    Typical usage::

        from valoricore.verify import AnchorVerifier

        anchor = AnchorVerifier.load("events.anchor")
        anchor.check_against_node(client)   # raises TamperDetected if mismatch

    The constructor does NOT verify the signature — call :meth:`verify_signature`
    or use :meth:`load` / :meth:`from_dict` with ``verify=True`` (the default).
    """

    chain_head: bytes        # 32 bytes — BLAKE3 chain head at anchor time
    event_count: int
    state_hash: bytes        # 32 bytes — BLAKE3 state hash at anchor time
    anchored_at_unix: int
    anchored_at: str         # ISO-8601 UTC string, e.g. "2026-06-10T14:02:11Z"
    public_key_bytes: bytes  # 32 bytes — Ed25519 verifying key
    signature_bytes: bytes   # 64 bytes — Ed25519 signature
    note: Optional[str] = None

    # ── constructors ──────────────────────────────────────────────────────────

    @classmethod
    def load(cls, path: Union[str, Path], *, verify: bool = True) -> "AnchorVerifier":
        """Load from a ``.anchor`` JSON file and optionally verify the signature."""
        with open(path) as fh:
            data = json.load(fh)
        return cls.from_dict(data, verify=verify)

    @classmethod
    def from_dict(cls, data: Dict[str, Any], *, verify: bool = True) -> "AnchorVerifier":
        """Parse an anchor dict (e.g. from ``json.load``) and optionally verify."""
        def _hex(key: str, expected_len: int) -> bytes:
            raw = data.get(key, "")
            if not isinstance(raw, str) or len(raw) != expected_len * 2:
                raise ValueError(
                    f"Anchor field '{key}' must be {expected_len * 2} hex chars, got {raw!r}"
                )
            return bytes.fromhex(raw)

        anchor = cls(
            chain_head=_hex("chain_head", 32),
            event_count=int(data["event_count"]),
            state_hash=_hex("state_hash", 32),
            anchored_at_unix=int(data["anchored_at_unix"]),
            anchored_at=str(data.get("anchored_at", "")),
            public_key_bytes=_hex("public_key_ed25519", 32),
            signature_bytes=_hex("signature_ed25519", 64),
            note=data.get("note"),
        )
        if verify:
            anchor.verify_signature()
        return anchor

    # ── core methods ──────────────────────────────────────────────────────────

    def _message(self) -> bytes:
        """Build the 97-byte signed message (must match the Rust anchor writer)."""
        return (
            _ANCHOR_DOMAIN_SEP
            + self.chain_head
            + struct.pack("<Q", self.event_count)
            + self.state_hash
            + struct.pack("<Q", self.anchored_at_unix)
        )

    def verify_signature(self) -> None:
        """
        Verify the Ed25519 signature over the anchor payload.
        Raises :class:`~valoricore.exceptions.IntegrityError` if invalid.
        """
        _ed25519_verify(self.public_key_bytes, self.signature_bytes, self._message())

    def check_against_node(self, client: Any, *, verify_signature: bool = True) -> None:
        """
        Compare this anchor against a live node's current state hash.

        :param client: Any client that exposes ``get_state_hash() -> str``
                       (``SyncRemoteClient``, ``LocalClient``, etc.).
        :param verify_signature: Re-verify the Ed25519 signature before comparing
                                 (default ``True``; set ``False`` if you already called
                                 :meth:`verify_signature`).

        Raises :class:`~valoricore.exceptions.TamperDetected` if the hashes differ.
        """
        if verify_signature:
            self.verify_signature()
        live = client.get_state_hash()
        expected = self.state_hash.hex()
        if live != expected:
            raise TamperDetected(
                f"State hash mismatch — the log has changed since it was anchored.\n"
                f"  anchored at:  {self.anchored_at}\n"
                f"  anchor hash:  {expected}\n"
                f"  live hash:    {live}\n"
                "Run valori-verify for a detailed forensic report."
            )

    async def async_check_against_node(
        self, client: Any, *, verify_signature: bool = True
    ) -> None:
        """Async variant of :meth:`check_against_node` for ``AsyncRemoteClient``."""
        if verify_signature:
            self.verify_signature()
        live = await client.get_state_hash()
        expected = self.state_hash.hex()
        if live != expected:
            raise TamperDetected(
                f"State hash mismatch — the log has changed since it was anchored.\n"
                f"  anchored at:  {self.anchored_at}\n"
                f"  anchor hash:  {expected}\n"
                f"  live hash:    {live}\n"
                "Run valori-verify for a detailed forensic report."
            )

    # ── convenience ───────────────────────────────────────────────────────────

    @property
    def chain_head_hex(self) -> str:
        return self.chain_head.hex()

    @property
    def state_hash_hex(self) -> str:
        return self.state_hash.hex()

    @property
    def public_key_hex(self) -> str:
        return self.public_key_bytes.hex()

    def __repr__(self) -> str:
        return (
            f"AnchorVerifier("
            f"events={self.event_count}, "
            f"anchored_at={self.anchored_at!r}, "
            f"state_hash={self.state_hash_hex[:16]}…)"
        )


# ── VerifyReport / TamperFinding ──────────────────────────────────────────────

@dataclass
class TamperFinding:
    """
    Structured representation of a tamper finding from ``valori-verify --report``.

    The ``type`` field determines which other fields are populated:

    - ``"chain_breach"``   — per-entry hash chain broke; entry number and payload known
    - ``"structural"``     — bincode decode failure; entry number and offset known
    - ``"semantic"``       — kernel rejected an entry; error detail known
    - ``"content"``        — chain intact but state hash mismatch
    """

    type: str

    # chain_breach fields
    breach_entry_no: Optional[int] = None
    breach_byte_offset: Optional[int] = None
    likely_altered_entry_no: Optional[int] = None
    likely_altered_entry_payload: Optional[str] = None
    breach_entry_committed: Optional[str] = None
    breach_entry_committed_unix: Optional[int] = None
    computed_chain_head: Optional[str] = None
    stored_prev_hash: Optional[str] = None
    events_clean_before_breach: Optional[int] = None

    # structural fields
    failed_entry_no: Optional[int] = None
    failed_byte_offset: Optional[int] = None
    trailing_unreadable_bytes: Optional[int] = None
    events_clean_before_failure: Optional[int] = None

    # semantic fields
    rejected_entry_no: Optional[int] = None
    rejected_byte_offset: Optional[int] = None
    kernel_error: Optional[str] = None
    events_clean_before_rejection: Optional[int] = None

    # content fields
    expected_state_hash: Optional[str] = None
    computed_state_hash: Optional[str] = None
    note: Optional[str] = None

    @classmethod
    def from_dict(cls, data: Dict[str, Any]) -> "TamperFinding":
        known = {f.name for f in cls.__dataclass_fields__.values()}  # type: ignore[attr-defined]
        filtered = {k: v for k, v in data.items() if k in known}
        return cls(**filtered)

    def summary(self) -> str:
        """One-line human summary of the finding."""
        if self.type == "chain_breach":
            return (
                f"chain breach at entry #{self.breach_entry_no} — "
                f"entry #{self.likely_altered_entry_no} was altered "
                f"(committed {self.breach_entry_committed})"
            )
        if self.type == "structural":
            return (
                f"structural corruption at entry #{self.failed_entry_no} "
                f"(byte offset {self.failed_byte_offset})"
            )
        if self.type == "semantic":
            return f"semantic rejection at entry #{self.rejected_entry_no}: {self.kernel_error}"
        if self.type == "content":
            return (
                f"state hash mismatch — expected {(self.expected_state_hash or '')[:16]}…, "
                f"got {(self.computed_state_hash or '')[:16]}…"
            )
        return f"unknown finding type: {self.type}"


@dataclass
class VerifyReport:
    """
    Structured result of a ``valori-verify --report`` run.

    Obtain via :func:`verify_log` or :meth:`from_file`.

    Quick usage::

        report = verify_log("/data/events.log", expected_hash=client.get_state_hash())
        if not report.is_verified:
            raise RuntimeError(f"Tamper detected: {report.finding.summary()}")
    """

    schema_version: int
    verdict: str
    log_path: str
    log_size_bytes: int
    format_version: int
    dim: int
    events_replayed: int
    checkpoints_seen: int
    state_hash: str
    chain_head: str
    expected_hash: Optional[str]
    generated_at: str
    generated_at_unix: int
    finding: Optional[TamperFinding]

    # ── constructors ──────────────────────────────────────────────────────────

    @classmethod
    def from_file(cls, path: Union[str, Path]) -> "VerifyReport":
        """Parse a report JSON file written by ``valori-verify --report``."""
        with open(path) as fh:
            return cls.from_dict(json.load(fh))

    @classmethod
    def from_dict(cls, data: Dict[str, Any]) -> "VerifyReport":
        log = data.get("log", {})
        replay = data.get("replay", {})
        finding_raw = data.get("finding")
        finding = TamperFinding.from_dict(finding_raw) if finding_raw else None
        return cls(
            schema_version=int(data.get("schema_version", 1)),
            verdict=str(data.get("verdict", "")),
            log_path=str(log.get("path", "")),
            log_size_bytes=int(log.get("size_bytes", 0)),
            format_version=int(log.get("format_version", 0)),
            dim=int(log.get("dim", 0)),
            events_replayed=int(replay.get("events_replayed", 0)),
            checkpoints_seen=int(replay.get("checkpoints_seen", 0)),
            state_hash=str(replay.get("state_hash", "")),
            chain_head=str(replay.get("chain_head", "")),
            expected_hash=data.get("expected_hash"),
            generated_at=str(data.get("generated_at", "")),
            generated_at_unix=int(data.get("generated_at_unix", 0)),
            finding=finding,
        )

    # ── convenience ───────────────────────────────────────────────────────────

    @property
    def is_verified(self) -> bool:
        """``True`` iff verdict is ``"verified"``."""
        return self.verdict == "verified"

    @property
    def is_tampered(self) -> bool:
        """``True`` iff any tamper verdict was reached."""
        return self.verdict.startswith("tampered_")

    def raise_if_tampered(self) -> None:
        """
        Raise :class:`~valoricore.exceptions.TamperDetected` if the log was
        tampered, with the finding summary as the message.  No-op if verified.
        """
        if self.is_tampered:
            detail = self.finding.summary() if self.finding else self.verdict
            raise TamperDetected(
                f"Tamper detected in {self.log_path}: {detail}"
            )

    def __repr__(self) -> str:
        return (
            f"VerifyReport(verdict={self.verdict!r}, "
            f"events={self.events_replayed}, "
            f"log={self.log_path!r})"
        )


# ── verify_log ────────────────────────────────────────────────────────────────

def _verify_via_ffi(log_path: Path, expected_hash: Optional[str]) -> Optional["VerifyReport"]:
    """
    Try to run verification through the in-process FFI extension (.so).

    Returns a VerifyReport on success, or None if the FFI is not available
    (e.g. pip install without the compiled wheel).
    """
    try:
        from valoricore_ffi import verify_log_file as _ffi_verify  # type: ignore[import]
        json_str = _ffi_verify(str(log_path), expected_hash)
        return VerifyReport.from_dict(json.loads(json_str))
    except ImportError:
        return None


def verify_log(
    log_path: Union[str, Path],
    expected_hash: Optional[str] = None,
    report_path: Optional[Union[str, Path]] = None,
    *,
    binary: Union[str, Path] = "valori-verify",
    trace: bool = False,
    raise_on_tamper: bool = False,
) -> VerifyReport:
    """
    Replay an ``events.log`` file and return a :class:`VerifyReport`.

    **No binary required for pip users.** When the compiled FFI extension
    (``valoricore_ffi``) is present in the process — which it is after a
    standard ``pip install valoricore`` — verification runs entirely in-process
    through the already-loaded ``.so``.  The ``valori-verify`` subprocess is
    only tried as a fallback (e.g. when running against a plain source checkout
    without having built the wheel).

    :param log_path:        Path to the ``events.log`` file.
    :param expected_hash:   64-char hex BLAKE3 state hash to compare against
                            (e.g. from ``client.get_state_hash()``).
    :param report_path:     If given, the JSON report is also written here.
                            Ignored when the FFI path is used (report is
                            returned in-memory only).
    :param binary:          Path to the ``valori-verify`` binary.  Only used
                            when the FFI is unavailable.  Defaults to
                            ``"valori-verify"`` (must be on PATH in that case).
    :param trace:           When using the subprocess fallback, pass ``--trace``
                            to print each event as it replays.
    :param raise_on_tamper: If ``True``, call :meth:`VerifyReport.raise_if_tampered`
                            before returning so callers need not check the verdict.

    :raises FileNotFoundError: if neither the FFI nor the binary is available.
    :raises RuntimeError:      if the binary exits without producing a valid report.
    :raises TamperDetected:    if ``raise_on_tamper=True`` and tampering is found.

    Example::

        report = verify_log(
            "/data/events.log",
            expected_hash=client.get_state_hash(),
            raise_on_tamper=True,
        )
        print(f"Verified {report.events_replayed} events, chain head: {report.chain_head}")
    """
    log_path = Path(log_path)

    # ── Fast path: in-process FFI (always available after pip install) ─────────
    ffi_report = _verify_via_ffi(log_path, expected_hash)
    if ffi_report is not None:
        if report_path is not None:
            with open(report_path, "w") as fh:
                json.dump(
                    {
                        "schema_version": ffi_report.schema_version,
                        "verdict": ffi_report.verdict,
                        "log": {
                            "path": ffi_report.log_path,
                            "size_bytes": ffi_report.log_size_bytes,
                            "format_version": ffi_report.format_version,
                            "dim": ffi_report.dim,
                        },
                        "replay": {
                            "events_replayed": ffi_report.events_replayed,
                            "checkpoints_seen": ffi_report.checkpoints_seen,
                            "state_hash": ffi_report.state_hash,
                            "chain_head": ffi_report.chain_head,
                        },
                        "expected_hash": ffi_report.expected_hash,
                        "generated_at": ffi_report.generated_at,
                        "generated_at_unix": ffi_report.generated_at_unix,
                    },
                    fh,
                    indent=2,
                )
        if raise_on_tamper:
            ffi_report.raise_if_tampered()
        return ffi_report

    # ── Fallback: subprocess (source-checkout / CI without wheel) ─────────────

    # M-2: validate binary to prevent arbitrary code execution when the parameter
    # comes from user input.  Accept a simple name (PATH lookup) or absolute path.
    binary_path = Path(binary)
    if not binary_path.is_absolute() and os.sep in str(binary):
        raise ValueError(
            f"binary must be a simple name (e.g. 'valori-verify') or an absolute path; "
            f"got {binary!r}. Relative paths with directory components are not allowed."
        )

    own_temp = report_path is None
    if own_temp:
        fd, tmp = tempfile.mkstemp(suffix=".json", prefix="valori_report_")
        os.close(fd)
        effective_report = tmp
    else:
        effective_report = str(report_path)

    cmd = [str(binary), str(log_path)]
    if expected_hash:
        cmd += ["--expected-hash", expected_hash]
    if trace:
        cmd.append("--trace")
    cmd += ["--report", effective_report]

    try:
        subprocess.run(cmd, check=False)  # non-zero exit = tampered, handled below
    except FileNotFoundError:
        raise FileNotFoundError(
            "The valoricore FFI extension is not loaded and the valori-verify binary "
            f"was not found at {binary!r}.\n"
            "\n"
            "For pip users — reinstall with the compiled wheel:\n"
            "    pip install --force-reinstall valoricore\n"
            "\n"
            "For source-checkout users — build the binary once:\n"
            "    cargo build -p valori-verify --release\n"
            "    export PATH=$PATH:$(pwd)/target/release\n"
            "\n"
            "Or pass the full path explicitly:\n"
            "    verify_log(path, binary='/path/to/valori-verify')"
        )

    try:
        with open(effective_report) as fh:
            data = json.load(fh)
    except (FileNotFoundError, json.JSONDecodeError) as exc:
        raise RuntimeError(
            f"valori-verify did not produce a valid JSON report at {effective_report!r}. "
            "Check that the binary version supports --report."
        ) from exc
    finally:
        if own_temp:
            try:
                os.unlink(effective_report)
            except OSError:
                pass

    report = VerifyReport.from_dict(data)
    if raise_on_tamper:
        report.raise_if_tampered()
    return report
