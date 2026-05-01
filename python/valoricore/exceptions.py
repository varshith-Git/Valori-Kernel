# Copyright (c) 2025 Varshith Gudur. Licensed under AGPLv3.

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
