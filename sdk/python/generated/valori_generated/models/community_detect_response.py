from collections.abc import Mapping
from typing import (
    TYPE_CHECKING,
    Any,
    TypeVar,
)

from attrs import define as _attrs_define
from attrs import field as _attrs_field

if TYPE_CHECKING:
    from ..models.community_summary import CommunitySummary


T = TypeVar("T", bound="CommunityDetectResponse")


@_attrs_define
class CommunityDetectResponse:
    """
    Attributes:
        communities (list['CommunitySummary']):
        community_count (int):
        node_count (int):
        receipt (str): BLAKE3 hex receipt over sorted assignments.
    """

    communities: list["CommunitySummary"]
    community_count: int
    node_count: int
    receipt: str
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        communities = []
        for communities_item_data in self.communities:
            communities_item = communities_item_data.to_dict()
            communities.append(communities_item)

        community_count = self.community_count

        node_count = self.node_count

        receipt = self.receipt

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "communities": communities,
                "community_count": community_count,
                "node_count": node_count,
                "receipt": receipt,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        from ..models.community_summary import CommunitySummary

        d = dict(src_dict)
        communities = []
        _communities = d.pop("communities")
        for communities_item_data in _communities:
            communities_item = CommunitySummary.from_dict(communities_item_data)

            communities.append(communities_item)

        community_count = d.pop("community_count")

        node_count = d.pop("node_count")

        receipt = d.pop("receipt")

        community_detect_response = cls(
            communities=communities,
            community_count=community_count,
            node_count=node_count,
            receipt=receipt,
        )

        community_detect_response.additional_properties = d
        return community_detect_response

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
