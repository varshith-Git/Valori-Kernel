from enum import Enum


class IndexKindInput(str, Enum):
    AUTO = "auto"
    BQ = "bq"
    BRUTE = "brute"
    BRUTEFORCE = "bruteforce"
    HNSW = "hnsw"
    IVF = "ivf"
    MSTG = "mstg"

    def __str__(self) -> str:
        return str(self.value)
