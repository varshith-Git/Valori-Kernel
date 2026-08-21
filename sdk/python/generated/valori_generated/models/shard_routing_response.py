from collections.abc import Mapping
from typing import (
    TYPE_CHECKING,
    Any,
    TypeVar,
)

from attrs import define as _attrs_define
from attrs import field as _attrs_field

if TYPE_CHECKING:
    from ..models.shard_routing_entry import ShardRoutingEntry


T = TypeVar("T", bound="ShardRoutingResponse")


@_attrs_define
class ShardRoutingResponse:
    """`GET /v1/shard/routing` — which collection lives on which logical shard.
    Routing is `namespace_id % shard_count`.

        Attributes:
            mode (str): `standalone` or `cluster`.
            shard_count (int):
            shards (list['ShardRoutingEntry']):
    """

    mode: str
    shard_count: int
    shards: list["ShardRoutingEntry"]
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        mode = self.mode

        shard_count = self.shard_count

        shards = []
        for shards_item_data in self.shards:
            shards_item = shards_item_data.to_dict()
            shards.append(shards_item)

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "mode": mode,
                "shard_count": shard_count,
                "shards": shards,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        from ..models.shard_routing_entry import ShardRoutingEntry

        d = dict(src_dict)
        mode = d.pop("mode")

        shard_count = d.pop("shard_count")

        shards = []
        _shards = d.pop("shards")
        for shards_item_data in _shards:
            shards_item = ShardRoutingEntry.from_dict(shards_item_data)

            shards.append(shards_item)

        shard_routing_response = cls(
            mode=mode,
            shard_count=shard_count,
            shards=shards,
        )

        shard_routing_response.additional_properties = d
        return shard_routing_response

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
