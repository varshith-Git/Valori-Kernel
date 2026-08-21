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

T = TypeVar("T", bound="TreeNode")


@_attrs_define
class TreeNode:
    """One section of a document — a node in the table-of-contents tree.

    Attributes:
        end_line (int): Last line owned by this section (excluding children).
        level (int):
        node_id (str):
        own_text (str): Verbatim section body, excluding sub-sections.
        start_line (int): 1-indexed line where this heading appears.
        summary (str): First sentence of the body — a no-LLM summary.
        title (str):
        children (Union[Unset, list[str]]):
        parent (Union[None, Unset, str]):
    """

    end_line: int
    level: int
    node_id: str
    own_text: str
    start_line: int
    summary: str
    title: str
    children: Union[Unset, list[str]] = UNSET
    parent: Union[None, Unset, str] = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        end_line = self.end_line

        level = self.level

        node_id = self.node_id

        own_text = self.own_text

        start_line = self.start_line

        summary = self.summary

        title = self.title

        children: Union[Unset, list[str]] = UNSET
        if not isinstance(self.children, Unset):
            children = self.children

        parent: Union[None, Unset, str]
        if isinstance(self.parent, Unset):
            parent = UNSET
        else:
            parent = self.parent

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "end_line": end_line,
                "level": level,
                "node_id": node_id,
                "own_text": own_text,
                "start_line": start_line,
                "summary": summary,
                "title": title,
            }
        )
        if children is not UNSET:
            field_dict["children"] = children
        if parent is not UNSET:
            field_dict["parent"] = parent

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)
        end_line = d.pop("end_line")

        level = d.pop("level")

        node_id = d.pop("node_id")

        own_text = d.pop("own_text")

        start_line = d.pop("start_line")

        summary = d.pop("summary")

        title = d.pop("title")

        children = cast(list[str], d.pop("children", UNSET))

        def _parse_parent(data: object) -> Union[None, Unset, str]:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(Union[None, Unset, str], data)

        parent = _parse_parent(d.pop("parent", UNSET))

        tree_node = cls(
            end_line=end_line,
            level=level,
            node_id=node_id,
            own_text=own_text,
            start_line=start_line,
            summary=summary,
            title=title,
            children=children,
            parent=parent,
        )

        tree_node.additional_properties = d
        return tree_node

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
