from collections.abc import Mapping
from typing import Any, TypeVar

from attrs import define as _attrs_define
from attrs import field as _attrs_field

T = TypeVar("T", bound="OperationMetrics")


@_attrs_define
class OperationMetrics:
    """The `metrics` block of [`OperationDetailResponse`].

    Attributes:
        cpu_cycles (int):
        duration_ms (float):
        memory_bytes (int):
        status (str): Qualitative assessment, e.g. `optimal`.
    """

    cpu_cycles: int
    duration_ms: float
    memory_bytes: int
    status: str
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        cpu_cycles = self.cpu_cycles

        duration_ms = self.duration_ms

        memory_bytes = self.memory_bytes

        status = self.status

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "cpu_cycles": cpu_cycles,
                "duration_ms": duration_ms,
                "memory_bytes": memory_bytes,
                "status": status,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)
        cpu_cycles = d.pop("cpu_cycles")

        duration_ms = d.pop("duration_ms")

        memory_bytes = d.pop("memory_bytes")

        status = d.pop("status")

        operation_metrics = cls(
            cpu_cycles=cpu_cycles,
            duration_ms=duration_ms,
            memory_bytes=memory_bytes,
            status=status,
        )

        operation_metrics.additional_properties = d
        return operation_metrics

    @property
    def additional_keys(self) -> list[str]:
        return list(self.additional_properties.keys())

    def __getitem__(self, key: str) -> Any:
        return self.additional_properties[key]

    def __setitem__(self, key: str, value: Any) -> None:
        self.additional_properties[key] = value

    def __delitem__(self, key: str) -> None:
        del self.additional_properties[key]

    def __contains__(self, key: str) -> bool:
        return key in self.additional_properties
