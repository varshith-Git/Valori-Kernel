from collections.abc import Mapping
from typing import (
    TYPE_CHECKING,
    Any,
    TypeVar,
)

from attrs import define as _attrs_define
from attrs import field as _attrs_field

if TYPE_CHECKING:
    from ..models.package_health import PackageHealth


T = TypeVar("T", bound="SystemHealth")


@_attrs_define
class SystemHealth:
    """Aggregate health report for the entire model package store.

    Returned by `GET /v1/models/health`.

        Attributes:
            corrupted (int):
            disk_used_bytes (int):
            missing (int):
            packages (list['PackageHealth']):
            reclaimable_bytes (int): Bytes used by packages with zero active references.
            total_installed (int):
            verified (int):
    """

    corrupted: int
    disk_used_bytes: int
    missing: int
    packages: list["PackageHealth"]
    reclaimable_bytes: int
    total_installed: int
    verified: int
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        corrupted = self.corrupted

        disk_used_bytes = self.disk_used_bytes

        missing = self.missing

        packages = []
        for packages_item_data in self.packages:
            packages_item = packages_item_data.to_dict()
            packages.append(packages_item)

        reclaimable_bytes = self.reclaimable_bytes

        total_installed = self.total_installed

        verified = self.verified

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "corrupted": corrupted,
                "disk_used_bytes": disk_used_bytes,
                "missing": missing,
                "packages": packages,
                "reclaimable_bytes": reclaimable_bytes,
                "total_installed": total_installed,
                "verified": verified,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        from ..models.package_health import PackageHealth

        d = dict(src_dict)
        corrupted = d.pop("corrupted")

        disk_used_bytes = d.pop("disk_used_bytes")

        missing = d.pop("missing")

        packages = []
        _packages = d.pop("packages")
        for packages_item_data in _packages:
            packages_item = PackageHealth.from_dict(packages_item_data)

            packages.append(packages_item)

        reclaimable_bytes = d.pop("reclaimable_bytes")

        total_installed = d.pop("total_installed")

        verified = d.pop("verified")

        system_health = cls(
            corrupted=corrupted,
            disk_used_bytes=disk_used_bytes,
            missing=missing,
            packages=packages,
            reclaimable_bytes=reclaimable_bytes,
            total_installed=total_installed,
            verified=verified,
        )

        system_health.additional_properties = d
        return system_health

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
