from enum import Enum


class IndexKind(str, Enum):
    AUTO = "auto"
    BQ = "bq"
    BRUTE = "brute"
    HNSW = "hnsw"
    IVF = "ivf"

    def __str__(self) -> str:
        return str(self.value)
