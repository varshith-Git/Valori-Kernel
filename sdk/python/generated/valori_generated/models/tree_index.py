from collections.abc import Mapping
from typing import (
    TYPE_CHECKING,
    Any,
    TypeVar,
    cast,
)

from attrs import define as _attrs_define
from attrs import field as _attrs_field

if TYPE_CHECKING:
    from ..models.tree_index_nodes import TreeIndexNodes


T = TypeVar("T", bound="TreeIndex")


@_attrs_define
class TreeIndex:
    """A hierarchical, line-addressable index of one document.

    Attributes:
        doc_name (str):
        nodes (TreeIndexNodes):
        roots (list[str]):
    """

    doc_name: str
    nodes: "TreeIndexNodes"
    roots: list[str]
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        doc_name = self.doc_name

        nodes = self.nodes.to_dict()

        roots = self.roots

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "doc_name": doc_name,
                "nodes": nodes,
                "roots": roots,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        from ..models.tree_index_nodes import TreeIndexNodes

        d = dict(src_dict)
        doc_name = d.pop("doc_name")

        nodes = TreeIndexNodes.from_dict(d.pop("nodes"))

        roots = cast(list[str], d.pop("roots"))

        tree_index = cls(
            doc_name=doc_name,
            nodes=nodes,
            roots=roots,
        )

        tree_index.additional_properties = d
        return tree_index

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
