from enum import Enum


class MetricInput(str, Enum):
    L2 = "l2"
    L2SQ = "l2sq"
    SQUARED_L2 = "squared_l2"

    def __str__(self) -> str:
        return str(self.value)
