from collections.abc import Mapping
from typing import Any, TypeVar

from attrs import define as _attrs_define
from attrs import field as _attrs_field

T = TypeVar("T", bound="UsageStorage")


@_attrs_define
class UsageStorage:
    """The `storage` sub-object of `GET /v1/usage`.

    Attributes:
        event_log_bytes (int): Live event-log segment plus every rotated archive segment.
        snapshot_bytes (int):
        total_bytes (int):
    """

    event_log_bytes: int
    snapshot_bytes: int
    total_bytes: int
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        event_log_bytes = self.event_log_bytes

        snapshot_bytes = self.snapshot_bytes

        total_bytes = self.total_bytes

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "event_log_bytes": event_log_bytes,
                "snapshot_bytes": snapshot_bytes,
                "total_bytes": total_bytes,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)
        event_log_bytes = d.pop("event_log_bytes")

        snapshot_bytes = d.pop("snapshot_bytes")

        total_bytes = d.pop("total_bytes")

        usage_storage = cls(
            event_log_bytes=event_log_bytes,
            snapshot_bytes=snapshot_bytes,
            total_bytes=total_bytes,
        )

        usage_storage.additional_properties = d
        return usage_storage

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
