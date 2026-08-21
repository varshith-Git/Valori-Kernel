from collections.abc import Mapping
from typing import (
    Any,
    TypeVar,
    cast,
)

from attrs import define as _attrs_define
from attrs import field as _attrs_field

T = TypeVar("T", bound="ShardRoutingEntry")


@_attrs_define
class ShardRoutingEntry:
    """One shard's collection assignment in `GET /v1/shard/routing`.

    Attributes:
        collections (list[str]):
        shard (int):
    """

    collections: list[str]
    shard: int
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        collections = self.collections

        shard = self.shard

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "collections": collections,
                "shard": shard,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)
        collections = cast(list[str], d.pop("collections"))

        shard = d.pop("shard")

        shard_routing_entry = cls(
            collections=collections,
            shard=shard,
        )

        shard_routing_entry.additional_properties = d
        return shard_routing_entry

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
