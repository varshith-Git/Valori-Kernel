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

T = TypeVar("T", bound="TreeChainVerifyResponse")


@_attrs_define
class TreeChainVerifyResponse:
    """
    Attributes:
        valid (bool):
        broken_at (Union[None, Unset, int]):
    """

    valid: bool
    broken_at: Union[None, Unset, int] = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        valid = self.valid

        broken_at: Union[None, Unset, int]
        if isinstance(self.broken_at, Unset):
            broken_at = UNSET
        else:
            broken_at = self.broken_at

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "valid": valid,
            }
        )
        if broken_at is not UNSET:
            field_dict["broken_at"] = broken_at

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)
        valid = d.pop("valid")

        def _parse_broken_at(data: object) -> Union[None, Unset, int]:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(Union[None, Unset, int], data)

        broken_at = _parse_broken_at(d.pop("broken_at", UNSET))

        tree_chain_verify_response = cls(
            valid=valid,
            broken_at=broken_at,
        )

        tree_chain_verify_response.additional_properties = d
        return tree_chain_verify_response

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
