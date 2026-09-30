from collections.abc import Mapping
from typing import (
    TYPE_CHECKING,
    Any,
    TypeVar,
    Union,
    cast,
)

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

if TYPE_CHECKING:
    from ..models.graph_rag_provenance_edge import GraphRagProvenanceEdge


T = TypeVar("T", bound="GraphRagProvenance")


@_attrs_define
class GraphRagProvenance:
    """
    Attributes:
        graph_path (list['GraphRagProvenanceEdge']):
        record_id (int):
        chunk_index (Union[None, Unset, int]):
        chunk_node_id (Union[None, Unset, int]):
        document_node_id (Union[None, Unset, int]):
        graph_distance (Union[None, Unset, int]):
        metadata_key (Union[None, Unset, str]):
        section_title (Union[None, Unset, str]):
        source (Union[None, Unset, str]):
    """

    graph_path: list["GraphRagProvenanceEdge"]
    record_id: int
    chunk_index: Union[None, Unset, int] = UNSET
    chunk_node_id: Union[None, Unset, int] = UNSET
    document_node_id: Union[None, Unset, int] = UNSET
    graph_distance: Union[None, Unset, int] = UNSET
    metadata_key: Union[None, Unset, str] = UNSET
    section_title: Union[None, Unset, str] = UNSET
    source: Union[None, Unset, str] = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        graph_path = []
        for graph_path_item_data in self.graph_path:
            graph_path_item = graph_path_item_data.to_dict()
            graph_path.append(graph_path_item)

        record_id = self.record_id

        chunk_index: Union[None, Unset, int]
        if isinstance(self.chunk_index, Unset):
            chunk_index = UNSET
        else:
            chunk_index = self.chunk_index

        chunk_node_id: Union[None, Unset, int]
        if isinstance(self.chunk_node_id, Unset):
            chunk_node_id = UNSET
        else:
            chunk_node_id = self.chunk_node_id

        document_node_id: Union[None, Unset, int]
        if isinstance(self.document_node_id, Unset):
            document_node_id = UNSET
        else:
            document_node_id = self.document_node_id

        graph_distance: Union[None, Unset, int]
        if isinstance(self.graph_distance, Unset):
            graph_distance = UNSET
        else:
            graph_distance = self.graph_distance

        metadata_key: Union[None, Unset, str]
        if isinstance(self.metadata_key, Unset):
            metadata_key = UNSET
        else:
            metadata_key = self.metadata_key

        section_title: Union[None, Unset, str]
        if isinstance(self.section_title, Unset):
            section_title = UNSET
        else:
            section_title = self.section_title

        source: Union[None, Unset, str]
        if isinstance(self.source, Unset):
            source = UNSET
        else:
            source = self.source

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "graph_path": graph_path,
                "record_id": record_id,
            }
        )
        if chunk_index is not UNSET:
            field_dict["chunk_index"] = chunk_index
        if chunk_node_id is not UNSET:
            field_dict["chunk_node_id"] = chunk_node_id
        if document_node_id is not UNSET:
            field_dict["document_node_id"] = document_node_id
        if graph_distance is not UNSET:
            field_dict["graph_distance"] = graph_distance
        if metadata_key is not UNSET:
            field_dict["metadata_key"] = metadata_key
        if section_title is not UNSET:
            field_dict["section_title"] = section_title
        if source is not UNSET:
            field_dict["source"] = source

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        from ..models.graph_rag_provenance_edge import GraphRagProvenanceEdge

        d = dict(src_dict)
        graph_path = []
        _graph_path = d.pop("graph_path")
        for graph_path_item_data in _graph_path:
            graph_path_item = GraphRagProvenanceEdge.from_dict(graph_path_item_data)

            graph_path.append(graph_path_item)

        record_id = d.pop("record_id")

        def _parse_chunk_index(data: object) -> Union[None, Unset, int]:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(Union[None, Unset, int], data)

        chunk_index = _parse_chunk_index(d.pop("chunk_index", UNSET))

        def _parse_chunk_node_id(data: object) -> Union[None, Unset, int]:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(Union[None, Unset, int], data)

        chunk_node_id = _parse_chunk_node_id(d.pop("chunk_node_id", UNSET))

        def _parse_document_node_id(data: object) -> Union[None, Unset, int]:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(Union[None, Unset, int], data)

        document_node_id = _parse_document_node_id(d.pop("document_node_id", UNSET))

        def _parse_graph_distance(data: object) -> Union[None, Unset, int]:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(Union[None, Unset, int], data)

        graph_distance = _parse_graph_distance(d.pop("graph_distance", UNSET))

        def _parse_metadata_key(data: object) -> Union[None, Unset, str]:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(Union[None, Unset, str], data)

        metadata_key = _parse_metadata_key(d.pop("metadata_key", UNSET))

        def _parse_section_title(data: object) -> Union[None, Unset, str]:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(Union[None, Unset, str], data)

        section_title = _parse_section_title(d.pop("section_title", UNSET))

        def _parse_source(data: object) -> Union[None, Unset, str]:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(Union[None, Unset, str], data)

        source = _parse_source(d.pop("source", UNSET))

        graph_rag_provenance = cls(
            graph_path=graph_path,
            record_id=record_id,
            chunk_index=chunk_index,
            chunk_node_id=chunk_node_id,
            document_node_id=document_node_id,
            graph_distance=graph_distance,
            metadata_key=metadata_key,
            section_title=section_title,
            source=source,
        )

        graph_rag_provenance.additional_properties = d
        return graph_rag_provenance

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
