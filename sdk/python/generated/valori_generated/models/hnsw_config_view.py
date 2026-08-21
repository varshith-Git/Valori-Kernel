from collections.abc import Mapping
from typing import Any, TypeVar

from attrs import define as _attrs_define
from attrs import field as _attrs_field

T = TypeVar("T", bound="HnswConfigView")


@_attrs_define
class HnswConfigView:
    """
    Attributes:
        ef_construction (int):
        ef_search (int):
        m (int):
        m_max0 (int):
    """

    ef_construction: int
    ef_search: int
    m: int
    m_max0: int
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        ef_construction = self.ef_construction

        ef_search = self.ef_search

        m = self.m

        m_max0 = self.m_max0

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "ef_construction": ef_construction,
                "ef_search": ef_search,
                "m": m,
                "m_max0": m_max0,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)
        ef_construction = d.pop("ef_construction")

        ef_search = d.pop("ef_search")

        m = d.pop("m")

        m_max0 = d.pop("m_max0")

        hnsw_config_view = cls(
            ef_construction=ef_construction,
            ef_search=ef_search,
            m=m,
            m_max0=m_max0,
        )

        hnsw_config_view.additional_properties = d
        return hnsw_config_view

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
