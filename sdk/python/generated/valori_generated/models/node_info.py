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

T = TypeVar("T", bound="NodeInfo")


@_attrs_define
class NodeInfo:
    """
    Attributes:
        kind (int):
        namespace_id (int):
        node_id (int):
        record_id (Union[None, Unset, int]):
    """

    kind: int
    namespace_id: int
    node_id: int
    record_id: Union[None, Unset, int] = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        kind = self.kind

        namespace_id = self.namespace_id

        node_id = self.node_id

        record_id: Union[None, Unset, int]
        if isinstance(self.record_id, Unset):
            record_id = UNSET
        else:
            record_id = self.record_id

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "kind": kind,
                "namespace_id": namespace_id,
                "node_id": node_id,
            }
        )
        if record_id is not UNSET:
            field_dict["record_id"] = record_id

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)
        kind = d.pop("kind")

        namespace_id = d.pop("namespace_id")

        node_id = d.pop("node_id")

        def _parse_record_id(data: object) -> Union[None, Unset, int]:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(Union[None, Unset, int], data)

        record_id = _parse_record_id(d.pop("record_id", UNSET))

        node_info = cls(
            kind=kind,
            namespace_id=namespace_id,
            node_id=node_id,
            record_id=record_id,
        )

        node_info.additional_properties = d
        return node_info

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
