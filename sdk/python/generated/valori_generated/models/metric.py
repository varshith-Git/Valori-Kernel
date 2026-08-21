from enum import Enum


class Metric(str, Enum):
    SQUARED_L2 = "squared_l2"

    def __str__(self) -> str:
        return str(self.value)
