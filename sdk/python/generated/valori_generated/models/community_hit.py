from collections.abc import Mapping
from typing import (
    Any,
    TypeVar,
    cast,
)

from attrs import define as _attrs_define
from attrs import field as _attrs_field

T = TypeVar("T", bound="CommunityHit")


@_attrs_define
class CommunityHit:
    """
    Attributes:
        community_id (int):
        member_count (int):
        sample_node_ids (list[int]):
        score (float):
    """

    community_id: int
    member_count: int
    sample_node_ids: list[int]
    score: float
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        community_id = self.community_id

        member_count = self.member_count

        sample_node_ids = self.sample_node_ids

        score = self.score

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "community_id": community_id,
                "member_count": member_count,
                "sample_node_ids": sample_node_ids,
                "score": score,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)
        community_id = d.pop("community_id")

        member_count = d.pop("member_count")

        sample_node_ids = cast(list[int], d.pop("sample_node_ids"))

        score = d.pop("score")

        community_hit = cls(
            community_id=community_id,
            member_count=member_count,
            sample_node_ids=sample_node_ids,
            score=score,
        )

        community_hit.additional_properties = d
        return community_hit

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
