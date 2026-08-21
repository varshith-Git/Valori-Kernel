from enum import Enum


class StageMetricsType0Stage(str, Enum):
    READER = "reader"

    def __str__(self) -> str:
        return str(self.value)
