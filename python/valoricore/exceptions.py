# Copyright (c) 2025 Varshith Gudur. Licensed under MIT OR Apache-2.0.

class ValoricoreError(Exception):
    """Base class for all Valoricore exceptions."""
    pass

class ConnectionError(ValoricoreError):
    """Raised when the client cannot connect to a remote Valoricore node."""
    pass

class ValidationError(ValoricoreError):
    """Raised when input data (e.g., vector dimensions) is invalid."""
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
