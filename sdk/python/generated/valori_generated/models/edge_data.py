from collections.abc import Mapping
from typing import Any, TypeVar

from attrs import define as _attrs_define
from attrs import field as _attrs_field

T = TypeVar("T", bound="EdgeData")


@_attrs_define
class EdgeData:
    """
    Attributes:
        edge_id (int):
        kind (int):
        to_node (int):
    """

    edge_id: int
    kind: int
    to_node: int
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        edge_id = self.edge_id

        kind = self.kind

        to_node = self.to_node

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "edge_id": edge_id,
                "kind": kind,
                "to_node": to_node,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)
        edge_id = d.pop("edge_id")

        kind = d.pop("kind")

        to_node = d.pop("to_node")

        edge_data = cls(
            edge_id=edge_id,
            kind=kind,
            to_node=to_node,
        )

        edge_data.additional_properties = d
        return edge_data

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
