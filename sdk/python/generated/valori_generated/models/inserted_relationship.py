from collections.abc import Mapping
from typing import Any, TypeVar

from attrs import define as _attrs_define
from attrs import field as _attrs_field

T = TypeVar("T", bound="InsertedRelationship")


@_attrs_define
class InsertedRelationship:
    """
    Attributes:
        description (str):
        edge_id (int):
        source_name (str):
        target_name (str):
    """

    description: str
    edge_id: int
    source_name: str
    target_name: str
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        description = self.description

        edge_id = self.edge_id

        source_name = self.source_name

        target_name = self.target_name

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "description": description,
                "edge_id": edge_id,
                "source_name": source_name,
                "target_name": target_name,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)
        description = d.pop("description")

        edge_id = d.pop("edge_id")

        source_name = d.pop("source_name")

        target_name = d.pop("target_name")

        inserted_relationship = cls(
            description=description,
            edge_id=edge_id,
            source_name=source_name,
            target_name=target_name,
        )

        inserted_relationship.additional_properties = d
        return inserted_relationship

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
