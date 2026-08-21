from collections.abc import Mapping
from typing import (
    TYPE_CHECKING,
    Any,
    TypeVar,
)

from attrs import define as _attrs_define
from attrs import field as _attrs_field

if TYPE_CHECKING:
    from ..models.wal_entry import WalEntry


T = TypeVar("T", bound="ListRemoteWalResponse")


@_attrs_define
class ListRemoteWalResponse:
    """
    Attributes:
        count (int):
        segments (list['WalEntry']):
    """

    count: int
    segments: list["WalEntry"]
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        count = self.count

        segments = []
        for segments_item_data in self.segments:
            segments_item = segments_item_data.to_dict()
            segments.append(segments_item)

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "count": count,
                "segments": segments,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        from ..models.wal_entry import WalEntry

        d = dict(src_dict)
        count = d.pop("count")

        segments = []
        _segments = d.pop("segments")
        for segments_item_data in _segments:
            segments_item = WalEntry.from_dict(segments_item_data)

            segments.append(segments_item)

        list_remote_wal_response = cls(
            count=count,
            segments=segments,
        )

        list_remote_wal_response.additional_properties = d
        return list_remote_wal_response

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
