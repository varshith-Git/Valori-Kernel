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

T = TypeVar("T", bound="CreateNodeResponse")


@_attrs_define
class CreateNodeResponse:
    """
    Attributes:
        node_id (int):
        log_index (Union[None, Unset, int]): Raft log index of the committed write — cluster path only.
    """

    node_id: int
    log_index: Union[None, Unset, int] = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        node_id = self.node_id

        log_index: Union[None, Unset, int]
        if isinstance(self.log_index, Unset):
            log_index = UNSET
        else:
            log_index = self.log_index

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "node_id": node_id,
            }
        )
        if log_index is not UNSET:
            field_dict["log_index"] = log_index

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)
        node_id = d.pop("node_id")

        def _parse_log_index(data: object) -> Union[None, Unset, int]:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(Union[None, Unset, int], data)

        log_index = _parse_log_index(d.pop("log_index", UNSET))

        create_node_response = cls(
            node_id=node_id,
            log_index=log_index,
        )

        create_node_response.additional_properties = d
        return create_node_response

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
