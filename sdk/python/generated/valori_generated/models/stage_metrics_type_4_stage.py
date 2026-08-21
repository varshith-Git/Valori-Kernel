from enum import Enum


class StageMetricsType4Stage(str, Enum):
    WRITER = "writer"

    def __str__(self) -> str:
        return str(self.value)
