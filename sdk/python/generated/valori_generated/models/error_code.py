from enum import Enum


class ErrorCode(str, Enum):
    CAPACITY_EXCEEDED = "capacity_exceeded"
    COLLECTION_NOT_FOUND = "collection_not_found"
    CONFLICT = "conflict"
    DIMENSION_MISMATCH = "dimension_mismatch"
    FORBIDDEN = "forbidden"
    INDEX_BUILD_FAILED = "index_build_failed"
    INTERNAL_ERROR = "internal_error"
    INVALID_INDEX = "invalid_index"
    INVALID_METRIC = "invalid_metric"
    NOT_FOUND = "not_found"
    NOT_IMPLEMENTED = "not_implemented"
    NOT_LEADER = "not_leader"
    RECORD_NOT_FOUND = "record_not_found"
    UNAUTHORIZED = "unauthorized"
    UNAVAILABLE = "unavailable"
    VALIDATION_ERROR = "validation_error"

    def __str__(self) -> str:
        return str(self.value)
