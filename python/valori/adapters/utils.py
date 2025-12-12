# Copyright (c) 2025 Varshith Gudur. Licensed under AGPLv3.
import numpy as np
from typing import List, Union

FXP_MAX = 32767.0
FXP_MIN = -32767.0
SCALE = 1 << 16

def validate_float_range(vec: Union[List[float], np.ndarray]) -> List[float]:
    """
    Validates and converts a float vector to Q16.16 compatible floats.
    Valori kernel expects floats but validates they are within safe range.
    Here we ensure they are finite and clamped/checked.
    """
    if isinstance(vec, list):
        vec = np.array(vec, dtype=np.float64)

from ..protocol import ValidationError

# ...

    if not np.isfinite(vec).all():
        raise ValidationError("Embedding contains non-finite values (NaN/Inf)")
    
    if vec.ndim != 1:
        raise ValidationError(f"Embedding must be 1D, got {vec.ndim}")

    if (vec > FXP_MAX).any() or (vec < FXP_MIN).any():
        # Option: clamp or raise? Prompt says "Reject ... > +32767" -> Raise.
        raise ValidationError(f"Embedding components must be within [{FXP_MIN}, {FXP_MAX}]")

    # Valori protocol currently takes List[float], not int raw.
    # The kernel does conversion. We just validate range here.
    # Prompt says "Reject if > 32767".
    # Return list of floats.
    return vec.tolist()
