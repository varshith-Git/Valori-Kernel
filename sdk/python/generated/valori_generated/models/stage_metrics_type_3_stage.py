from enum import Enum


class StageMetricsType3Stage(str, Enum):
    EMBEDDER = "embedder"

    def __str__(self) -> str:
        return str(self.value)
