from collections.abc import Mapping
from typing import (
    TYPE_CHECKING,
    Any,
    TypeVar,
)

from attrs import define as _attrs_define
from attrs import field as _attrs_field

if TYPE_CHECKING:
    from ..models.structure_node import StructureNode
    from ..models.tree_index import TreeIndex


T = TypeVar("T", bound="TreeBuildResponse")


@_attrs_define
class TreeBuildResponse:
    """
    Attributes:
        cache_key (str):
        doc_name (str):
        node_count (int):
        structure_map (list['StructureNode']):
        tree (TreeIndex): A hierarchical, line-addressable index of one document.
    """

    cache_key: str
    doc_name: str
    node_count: int
    structure_map: list["StructureNode"]
    tree: "TreeIndex"
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        cache_key = self.cache_key

        doc_name = self.doc_name

        node_count = self.node_count

        structure_map = []
        for structure_map_item_data in self.structure_map:
            structure_map_item = structure_map_item_data.to_dict()
            structure_map.append(structure_map_item)

        tree = self.tree.to_dict()

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "cache_key": cache_key,
                "doc_name": doc_name,
                "node_count": node_count,
                "structure_map": structure_map,
                "tree": tree,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        from ..models.structure_node import StructureNode
        from ..models.tree_index import TreeIndex

        d = dict(src_dict)
        cache_key = d.pop("cache_key")

        doc_name = d.pop("doc_name")

        node_count = d.pop("node_count")

        structure_map = []
        _structure_map = d.pop("structure_map")
        for structure_map_item_data in _structure_map:
            structure_map_item = StructureNode.from_dict(structure_map_item_data)

            structure_map.append(structure_map_item)

        tree = TreeIndex.from_dict(d.pop("tree"))

        tree_build_response = cls(
            cache_key=cache_key,
            doc_name=doc_name,
            node_count=node_count,
            structure_map=structure_map,
            tree=tree,
        )

        tree_build_response.additional_properties = d
        return tree_build_response

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
