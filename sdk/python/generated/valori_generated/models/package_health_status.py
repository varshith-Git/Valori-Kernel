from enum import Enum


class PackageHealthStatus(str, Enum):
    CORRUPTED = "corrupted"
    INSTALLED = "installed"
    MISSING = "missing"
    VERIFIED = "verified"

    def __str__(self) -> str:
        return str(self.value)
