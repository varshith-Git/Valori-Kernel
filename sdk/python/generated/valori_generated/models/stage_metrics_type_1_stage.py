from enum import Enum


class StageMetricsType1Stage(str, Enum):
    VALIDATOR = "validator"

    def __str__(self) -> str:
        return str(self.value)
