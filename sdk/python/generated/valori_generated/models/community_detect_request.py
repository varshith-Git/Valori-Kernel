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

T = TypeVar("T", bound="CommunityDetectRequest")


@_attrs_define
class CommunityDetectRequest:
    """
    Attributes:
        max_iter (Union[None, Unset, int]):
        namespace (Union[None, Unset, str]):
    """

    max_iter: Union[None, Unset, int] = UNSET
    namespace: Union[None, Unset, str] = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        max_iter: Union[None, Unset, int]
        if isinstance(self.max_iter, Unset):
            max_iter = UNSET
        else:
            max_iter = self.max_iter

        namespace: Union[None, Unset, str]
        if isinstance(self.namespace, Unset):
            namespace = UNSET
        else:
            namespace = self.namespace

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update({})
        if max_iter is not UNSET:
            field_dict["max_iter"] = max_iter
        if namespace is not UNSET:
            field_dict["namespace"] = namespace

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)

        def _parse_max_iter(data: object) -> Union[None, Unset, int]:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(Union[None, Unset, int], data)

        max_iter = _parse_max_iter(d.pop("max_iter", UNSET))

        def _parse_namespace(data: object) -> Union[None, Unset, str]:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(Union[None, Unset, str], data)

        namespace = _parse_namespace(d.pop("namespace", UNSET))

        community_detect_request = cls(
            max_iter=max_iter,
            namespace=namespace,
        )

        community_detect_request.additional_properties = d
        return community_detect_request

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
