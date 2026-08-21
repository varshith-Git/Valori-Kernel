from collections.abc import Mapping
from typing import Any, TypeVar

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..models.package_health_status import PackageHealthStatus

T = TypeVar("T", bound="PackageHealth")


@_attrs_define
class PackageHealth:
    """Per-package health entry.

    Attributes:
        id (str):
        ref_count (int):
        size_bytes (int):
        status (PackageHealthStatus): Health status for one installed package.
    """

    id: str
    ref_count: int
    size_bytes: int
    status: PackageHealthStatus
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        id = self.id

        ref_count = self.ref_count

        size_bytes = self.size_bytes

        status = self.status.value

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "id": id,
                "ref_count": ref_count,
                "size_bytes": size_bytes,
                "status": status,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)
        id = d.pop("id")

        ref_count = d.pop("ref_count")

        size_bytes = d.pop("size_bytes")

        status = PackageHealthStatus(d.pop("status"))

        package_health = cls(
            id=id,
            ref_count=ref_count,
            size_bytes=size_bytes,
            status=status,
        )

        package_health.additional_properties = d
        return package_health

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
