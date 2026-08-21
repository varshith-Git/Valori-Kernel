from enum import Enum


class StageMetricsType2Stage(str, Enum):
    CHUNKER = "chunker"

    def __str__(self) -> str:
        return str(self.value)
