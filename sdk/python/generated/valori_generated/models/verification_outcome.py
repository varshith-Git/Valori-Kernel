from enum import Enum


class VerificationOutcome(str, Enum):
    CONTRADICTS = "Contradicts"
    NEUTRAL = "Neutral"
    SUPPORTS = "Supports"
    UNKNOWN = "Unknown"

    def __str__(self) -> str:
        return str(self.value)
