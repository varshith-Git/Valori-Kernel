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

T = TypeVar("T", bound="CommunitySearchRequest")


@_attrs_define
class CommunitySearchRequest:
    """
    Attributes:
        vector (list[float]):
        depth (Union[Unset, int]):
        drill_in (Union[Unset, bool]):
        k (Union[Unset, int]):
        namespace (Union[None, Unset, str]):
    """

    vector: list[float]
    depth: Union[Unset, int] = UNSET
    drill_in: Union[Unset, bool] = UNSET
    k: Union[Unset, int] = UNSET
    namespace: Union[None, Unset, str] = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        vector = self.vector

        depth = self.depth

        drill_in = self.drill_in

        k = self.k

        namespace: Union[None, Unset, str]
        if isinstance(self.namespace, Unset):
            namespace = UNSET
        else:
            namespace = self.namespace

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "vector": vector,
            }
        )
        if depth is not UNSET:
            field_dict["depth"] = depth
        if drill_in is not UNSET:
            field_dict["drill_in"] = drill_in
        if k is not UNSET:
            field_dict["k"] = k
        if namespace is not UNSET:
            field_dict["namespace"] = namespace

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)
        vector = cast(list[float], d.pop("vector"))

        depth = d.pop("depth", UNSET)

        drill_in = d.pop("drill_in", UNSET)

        k = d.pop("k", UNSET)

        def _parse_namespace(data: object) -> Union[None, Unset, str]:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(Union[None, Unset, str], data)

        namespace = _parse_namespace(d.pop("namespace", UNSET))

        community_search_request = cls(
            vector=vector,
            depth=depth,
            drill_in=drill_in,
            k=k,
            namespace=namespace,
        )

        community_search_request.additional_properties = d
        return community_search_request

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
