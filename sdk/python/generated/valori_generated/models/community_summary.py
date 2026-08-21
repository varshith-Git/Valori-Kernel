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

T = TypeVar("T", bound="CommunitySummary")


@_attrs_define
class CommunitySummary:
    """
    Attributes:
        community_id (int):
        member_count (int):
        centroid_record_id (Union[None, Unset, int]):
    """

    community_id: int
    member_count: int
    centroid_record_id: Union[None, Unset, int] = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        community_id = self.community_id

        member_count = self.member_count

        centroid_record_id: Union[None, Unset, int]
        if isinstance(self.centroid_record_id, Unset):
            centroid_record_id = UNSET
        else:
            centroid_record_id = self.centroid_record_id

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "community_id": community_id,
                "member_count": member_count,
            }
        )
        if centroid_record_id is not UNSET:
            field_dict["centroid_record_id"] = centroid_record_id

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)
        community_id = d.pop("community_id")

        member_count = d.pop("member_count")

        def _parse_centroid_record_id(data: object) -> Union[None, Unset, int]:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(Union[None, Unset, int], data)

        centroid_record_id = _parse_centroid_record_id(
            d.pop("centroid_record_id", UNSET)
        )

        community_summary = cls(
            community_id=community_id,
            member_count=member_count,
            centroid_record_id=centroid_record_id,
        )

        community_summary.additional_properties = d
        return community_summary

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
