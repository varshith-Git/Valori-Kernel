import valori
from typing import List, Union
from ..protocol import ValidationError

def validate_float_range(vec: Union[List[float], "np.ndarray"]) -> List[float]:
    """
    Validates a float vector using the exact Rust kernel validation path.
    Guarantees that float validation is identical across Python and Rust.
    """
    if hasattr(vec, "tolist"):
        vec = vec.tolist()
        
    try:
        # Call the Rust FFI single source of truth for validation
        valori.ingest_embedding(vec)
        return vec
    except ValueError as e:
        raise ValidationError(str(e))
