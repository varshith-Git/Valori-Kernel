from collections.abc import Mapping
from typing import (
    TYPE_CHECKING,
    Any,
    TypeVar,
)

from attrs import define as _attrs_define
from attrs import field as _attrs_field

if TYPE_CHECKING:
    from ..models.community_hit import CommunityHit


T = TypeVar("T", bound="CommunitySearchResponse")


@_attrs_define
class CommunitySearchResponse:
    """
    Attributes:
        communities (list['CommunityHit']):
        total_communities_searched (int):
    """

    communities: list["CommunityHit"]
    total_communities_searched: int
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        communities = []
        for communities_item_data in self.communities:
            communities_item = communities_item_data.to_dict()
            communities.append(communities_item)

        total_communities_searched = self.total_communities_searched

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "communities": communities,
                "total_communities_searched": total_communities_searched,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        from ..models.community_hit import CommunityHit

        d = dict(src_dict)
        communities = []
        _communities = d.pop("communities")
        for communities_item_data in _communities:
            communities_item = CommunityHit.from_dict(communities_item_data)

            communities.append(communities_item)

        total_communities_searched = d.pop("total_communities_searched")

        community_search_response = cls(
            communities=communities,
            total_communities_searched=total_communities_searched,
        )

        community_search_response.additional_properties = d
        return community_search_response

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
