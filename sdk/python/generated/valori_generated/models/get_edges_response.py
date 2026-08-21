from collections.abc import Mapping
from typing import (
    TYPE_CHECKING,
    Any,
    TypeVar,
)

from attrs import define as _attrs_define
from attrs import field as _attrs_field

if TYPE_CHECKING:
    from ..models.edge_data import EdgeData


T = TypeVar("T", bound="GetEdgesResponse")


@_attrs_define
class GetEdgesResponse:
    """
    Attributes:
        edges (list['EdgeData']):
    """

    edges: list["EdgeData"]
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        edges = []
        for edges_item_data in self.edges:
            edges_item = edges_item_data.to_dict()
            edges.append(edges_item)

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "edges": edges,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        from ..models.edge_data import EdgeData

        d = dict(src_dict)
        edges = []
        _edges = d.pop("edges")
        for edges_item_data in _edges:
            edges_item = EdgeData.from_dict(edges_item_data)

            edges.append(edges_item)

        get_edges_response = cls(
            edges=edges,
        )

        get_edges_response.additional_properties = d
        return get_edges_response

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
