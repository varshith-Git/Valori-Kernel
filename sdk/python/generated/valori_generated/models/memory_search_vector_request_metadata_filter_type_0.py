from collections.abc import Mapping
from typing import Any, TypeVar

from attrs import define as _attrs_define
from attrs import field as _attrs_field

T = TypeVar("T", bound="MemorySearchVectorRequestMetadataFilterType0")


@_attrs_define
class MemorySearchVectorRequestMetadataFilterType0:
    """Phase I7 — restrict results to records whose stored metadata satisfies
    every key/value predicate. Same semantics as `SearchRequest::metadata_filter`.

    """

    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)
        memory_search_vector_request_metadata_filter_type_0 = cls()

        memory_search_vector_request_metadata_filter_type_0.additional_properties = d
        return memory_search_vector_request_metadata_filter_type_0

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
