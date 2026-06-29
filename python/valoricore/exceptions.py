# Copyright (c) 2025 Varshith Gudur. Licensed under MIT OR Apache-2.0.

class ValoricoreError(Exception):
    """Base class for all Valoricore exceptions."""
    pass

class ConnectionError(ValoricoreError):
    """Raised when the client cannot connect to a remote Valoricore node."""
    pass

class ValidationError(ValoricoreError, ValueError):
    """Raised when input data (e.g. vector dimensions or FXP bounds) is invalid."""
    pass

class ProtocolError(ValoricoreError):
    """Raised for protocol-level problems (unexpected server response shape, etc.)."""
    pass

class IntegrityError(ValoricoreError):
    """Raised when a cryptographic proof fails verification."""
    pass

class KernelError(ValoricoreError):
    """Raised when the underlying Rust kernel encounters an unrecoverable error."""
    pass

class NotFoundError(ValoricoreError):
    """Raised when a record, node, or edge does not exist."""
    pass

class AuthenticationError(ValoricoreError):
    """
    Raised when the server returns HTTP 401 (missing or invalid token) or
    HTTP 403 (token present but lacks permission for this operation).

    Set ``token=`` on the client or check ``VALORI_AUTH_TOKEN`` on the node.
    """
    pass

class NotLeaderError(ValoricoreError):
    """
    Raised when a write hits a cluster follower that cannot name the leader
    (e.g. during an election), and the client exhausts its retries. In a
    healthy cluster the client transparently follows the leader redirect, so
    this surfaces only when no leader is currently elected.
    """
    pass

class TamperDetected(IntegrityError):
    """
    Raised when a live node's state hash differs from an anchor, or when
    ``verify_log(..., raise_on_tamper=True)`` finds a tampered log.

    The message includes the finding summary and, where available, the
    specific event number, byte offset, and commit timestamp of the damage.
    """
    pass
