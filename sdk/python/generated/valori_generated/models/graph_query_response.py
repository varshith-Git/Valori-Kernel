from collections.abc import Mapping
from typing import (
    TYPE_CHECKING,
    Any,
    TypeVar,
)

from attrs import define as _attrs_define
from attrs import field as _attrs_field

if TYPE_CHECKING:
    from ..models.graph_query_hit_dto import GraphQueryHitDto


T = TypeVar("T", bound="GraphQueryResponse")


@_attrs_define
class GraphQueryResponse:
    """
    Attributes:
        count (int):
        hits (list['GraphQueryHitDto']):
    """

    count: int
    hits: list["GraphQueryHitDto"]
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        count = self.count

        hits = []
        for hits_item_data in self.hits:
            hits_item = hits_item_data.to_dict()
            hits.append(hits_item)

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "count": count,
                "hits": hits,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        from ..models.graph_query_hit_dto import GraphQueryHitDto

        d = dict(src_dict)
        count = d.pop("count")

        hits = []
        _hits = d.pop("hits")
        for hits_item_data in _hits:
            hits_item = GraphQueryHitDto.from_dict(hits_item_data)

            hits.append(hits_item)

        graph_query_response = cls(
            count=count,
            hits=hits,
        )

        graph_query_response.additional_properties = d
        return graph_query_response

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
