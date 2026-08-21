from collections.abc import Mapping
from typing import Any, TypeVar

from attrs import define as _attrs_define
from attrs import field as _attrs_field

T = TypeVar("T", bound="RestoreFromStoreResponse")


@_attrs_define
class RestoreFromStoreResponse:
    """
    Attributes:
        key (str):
        size_bytes (int):
        state_hash (str):
    """

    key: str
    size_bytes: int
    state_hash: str
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        key = self.key

        size_bytes = self.size_bytes

        state_hash = self.state_hash

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "key": key,
                "size_bytes": size_bytes,
                "state_hash": state_hash,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)
        key = d.pop("key")

        size_bytes = d.pop("size_bytes")

        state_hash = d.pop("state_hash")

        restore_from_store_response = cls(
            key=key,
            size_bytes=size_bytes,
            state_hash=state_hash,
        )

        restore_from_store_response.additional_properties = d
        return restore_from_store_response

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
