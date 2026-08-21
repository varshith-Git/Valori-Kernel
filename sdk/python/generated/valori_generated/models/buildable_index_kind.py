from enum import Enum


class BuildableIndexKind(str, Enum):
    BQ = "bq"
    HNSW = "hnsw"
    IVF = "ivf"

    def __str__(self) -> str:
        return str(self.value)
