# Copyright (c) 2025 Varshith Gudur. Licensed under MIT OR Apache-2.0.
from typing import List, Dict, Any, Union, Optional, TypeVar

# Core Type Aliases
Vector = List[float]
FixedVector = List[int]
Proof = bytes       # Raw BLAKE3 Merkle Proof
StateHash = str     # Hex-encoded state root
NodeId = int
RecordId = int
Tag = int

# Generic Client Type
T = TypeVar("T")
Metadata = Dict[str, Any]
