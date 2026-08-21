from collections.abc import Mapping
from typing import (
    Any,
    TypeVar,
    Union,
    cast,
)

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

T = TypeVar("T", bound="InsertedEntity")


@_attrs_define
class InsertedEntity:
    """
    Attributes:
        description (str):
        name (str):
        node_id (int):
        type_ (str):
        record_id (Union[None, Unset, int]):
    """

    description: str
    name: str
    node_id: int
    type_: str
    record_id: Union[None, Unset, int] = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        description = self.description

        name = self.name

        node_id = self.node_id

        type_ = self.type_

        record_id: Union[None, Unset, int]
        if isinstance(self.record_id, Unset):
            record_id = UNSET
        else:
            record_id = self.record_id

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "description": description,
                "name": name,
                "node_id": node_id,
                "type": type_,
            }
        )
        if record_id is not UNSET:
            field_dict["record_id"] = record_id

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)
        description = d.pop("description")

        name = d.pop("name")

        node_id = d.pop("node_id")

        type_ = d.pop("type")

        def _parse_record_id(data: object) -> Union[None, Unset, int]:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(Union[None, Unset, int], data)

        record_id = _parse_record_id(d.pop("record_id", UNSET))

        inserted_entity = cls(
            description=description,
            name=name,
            node_id=node_id,
            type_=type_,
            record_id=record_id,
        )

        inserted_entity.additional_properties = d
        return inserted_entity

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
