from collections.abc import Mapping
from typing import (
    Any,
    TypeVar,
    cast,
)

from attrs import define as _attrs_define
from attrs import field as _attrs_field

T = TypeVar("T", bound="TreeCitation")


@_attrs_define
class TreeCitation:
    """A citation back to the exact section + line range an answer came from.

    Attributes:
        breadcrumb (str):
        lines (list[int]):
        node_id (str):
        title (str):
    """

    breadcrumb: str
    lines: list[int]
    node_id: str
    title: str
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        breadcrumb = self.breadcrumb

        lines = self.lines

        node_id = self.node_id

        title = self.title

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "breadcrumb": breadcrumb,
                "lines": lines,
                "node_id": node_id,
                "title": title,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)
        breadcrumb = d.pop("breadcrumb")

        lines = cast(list[int], d.pop("lines"))

        node_id = d.pop("node_id")

        title = d.pop("title")

        tree_citation = cls(
            breadcrumb=breadcrumb,
            lines=lines,
            node_id=node_id,
            title=title,
        )

        tree_citation.additional_properties = d
        return tree_citation

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
