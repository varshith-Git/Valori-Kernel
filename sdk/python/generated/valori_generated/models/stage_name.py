from enum import Enum


class StageName(str, Enum):
    CHUNKER = "chunker"
    EMBEDDER = "embedder"
    READER = "reader"
    VALIDATOR = "validator"
    WRITER = "writer"

    def __str__(self) -> str:
        return str(self.value)
