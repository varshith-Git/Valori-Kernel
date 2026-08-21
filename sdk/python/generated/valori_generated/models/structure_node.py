from collections.abc import Mapping
from typing import (
    Any,
    TypeVar,
    Union,
)

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

T = TypeVar("T", bound="StructureNode")


@_attrs_define
class StructureNode:
    """A compact table-of-contents entry (title + summary, no body).

    Attributes:
        node_id (str):
        summary (str):
        title (str):
        nodes (Union[Unset, list['StructureNode']]): Child sections. `no_recursion` stops utoipa's schema builder from
            descending into this type forever — the generated document emits a
            `$ref` back to `StructureNode` instead of an infinitely nested inline
            schema.
    """

    node_id: str
    summary: str
    title: str
    nodes: Union[Unset, list["StructureNode"]] = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        node_id = self.node_id

        summary = self.summary

        title = self.title

        nodes: Union[Unset, list[dict[str, Any]]] = UNSET
        if not isinstance(self.nodes, Unset):
            nodes = []
            for nodes_item_data in self.nodes:
                nodes_item = nodes_item_data.to_dict()
                nodes.append(nodes_item)

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "node_id": node_id,
                "summary": summary,
                "title": title,
            }
        )
        if nodes is not UNSET:
            field_dict["nodes"] = nodes

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)
        node_id = d.pop("node_id")

        summary = d.pop("summary")

        title = d.pop("title")

        nodes = []
        _nodes = d.pop("nodes", UNSET)
        for nodes_item_data in _nodes or []:
            nodes_item = StructureNode.from_dict(nodes_item_data)

            nodes.append(nodes_item)

        structure_node = cls(
            node_id=node_id,
            summary=summary,
            title=title,
            nodes=nodes,
        )

        structure_node.additional_properties = d
        return structure_node

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
