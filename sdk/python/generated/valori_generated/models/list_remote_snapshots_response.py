from collections.abc import Mapping
from typing import (
    TYPE_CHECKING,
    Any,
    TypeVar,
)

from attrs import define as _attrs_define
from attrs import field as _attrs_field

if TYPE_CHECKING:
    from ..models.snapshot_entry import SnapshotEntry


T = TypeVar("T", bound="ListRemoteSnapshotsResponse")


@_attrs_define
class ListRemoteSnapshotsResponse:
    """
    Attributes:
        count (int):
        snapshots (list['SnapshotEntry']):
    """

    count: int
    snapshots: list["SnapshotEntry"]
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        count = self.count

        snapshots = []
        for snapshots_item_data in self.snapshots:
            snapshots_item = snapshots_item_data.to_dict()
            snapshots.append(snapshots_item)

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "count": count,
                "snapshots": snapshots,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        from ..models.snapshot_entry import SnapshotEntry

        d = dict(src_dict)
        count = d.pop("count")

        snapshots = []
        _snapshots = d.pop("snapshots")
        for snapshots_item_data in _snapshots:
            snapshots_item = SnapshotEntry.from_dict(snapshots_item_data)

            snapshots.append(snapshots_item)

        list_remote_snapshots_response = cls(
            count=count,
            snapshots=snapshots,
        )

        list_remote_snapshots_response.additional_properties = d
        return list_remote_snapshots_response

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
