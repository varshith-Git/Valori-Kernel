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
    from ..models.graph_rag_hit_metadata_type_0 import GraphRagHitMetadataType0


T = TypeVar("T", bound="GraphRagHit")


@_attrs_define
class GraphRagHit:
    """One blended vector + graph hit from `POST /v1/graphrag`.

    Phase API-3.3: `GraphRagResponse.hits` was `Vec<Object>`, so GraphRAG — a
    headline retrieval feature — returned `object[]` to every generated SDK.
    The producer is `capabilities.rs`, which builds a fixed ten-key object.

    Several scores are nullable by design: a hit reached purely through graph
    expansion has no vector distance, so `score` and `vector_score` are `null`
    on it. `final_score` and `graph_score` are always present.

        Attributes:
            final_score (float): Combined score in `[0, 1]`. Always present; rank on this.
            graph_score (float): Normalised graph relevance in `[0, 1]`.
            memory_id (str): Stable memory identity, `rec:<record_id>`.
            record_id (int): The underlying record.
            source (str): How this hit entered the result set — e.g. `vector`, `graph`.
            graph_distance (Union[None, Unset, int]): Hop count from the nearest seed node, when reachable.
            metadata (Union['GraphRagHitMetadataType0', None, Unset]): Caller-supplied metadata stored alongside the record,
                if any.
            node_id (Union[None, Unset, int]): Graph node for this record, when it has one.
            score (Union[None, Unset, float]): Vector distance. `null` for a graph-only hit. Retained for backward
                compatibility; `vector_score` is the explicit spelling of the same value.
            vector_score (Union[None, Unset, float]): Vector distance. `null` for a graph-only hit.
    """

    final_score: float
    graph_score: float
    memory_id: str
    record_id: int
    source: str
    graph_distance: Union[None, Unset, int] = UNSET
    metadata: Union["GraphRagHitMetadataType0", None, Unset] = UNSET
    node_id: Union[None, Unset, int] = UNSET
    score: Union[None, Unset, float] = UNSET
    vector_score: Union[None, Unset, float] = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        from ..models.graph_rag_hit_metadata_type_0 import GraphRagHitMetadataType0

        final_score = self.final_score

        graph_score = self.graph_score

        memory_id = self.memory_id

        record_id = self.record_id

        source = self.source

        graph_distance: Union[None, Unset, int]
        if isinstance(self.graph_distance, Unset):
            graph_distance = UNSET
        else:
            graph_distance = self.graph_distance

        metadata: Union[None, Unset, dict[str, Any]]
        if isinstance(self.metadata, Unset):
            metadata = UNSET
        elif isinstance(self.metadata, GraphRagHitMetadataType0):
            metadata = self.metadata.to_dict()
        else:
            metadata = self.metadata

        node_id: Union[None, Unset, int]
        if isinstance(self.node_id, Unset):
            node_id = UNSET
        else:
            node_id = self.node_id

        score: Union[None, Unset, float]
        if isinstance(self.score, Unset):
            score = UNSET
        else:
            score = self.score

        vector_score: Union[None, Unset, float]
        if isinstance(self.vector_score, Unset):
            vector_score = UNSET
        else:
            vector_score = self.vector_score

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "final_score": final_score,
                "graph_score": graph_score,
                "memory_id": memory_id,
                "record_id": record_id,
                "source": source,
            }
        )
        if graph_distance is not UNSET:
            field_dict["graph_distance"] = graph_distance
        if metadata is not UNSET:
            field_dict["metadata"] = metadata
        if node_id is not UNSET:
            field_dict["node_id"] = node_id
        if score is not UNSET:
            field_dict["score"] = score
        if vector_score is not UNSET:
            field_dict["vector_score"] = vector_score

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        from ..models.graph_rag_hit_metadata_type_0 import GraphRagHitMetadataType0

        d = dict(src_dict)
        final_score = d.pop("final_score")

        graph_score = d.pop("graph_score")

        memory_id = d.pop("memory_id")

        record_id = d.pop("record_id")

        source = d.pop("source")

        def _parse_graph_distance(data: object) -> Union[None, Unset, int]:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(Union[None, Unset, int], data)

        graph_distance = _parse_graph_distance(d.pop("graph_distance", UNSET))

        def _parse_metadata(
            data: object,
        ) -> Union["GraphRagHitMetadataType0", None, Unset]:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            try:
                if not isinstance(data, dict):
                    raise TypeError()
                metadata_type_0 = GraphRagHitMetadataType0.from_dict(data)

                return metadata_type_0
            except:  # noqa: E722
                pass
            return cast(Union["GraphRagHitMetadataType0", None, Unset], data)

        metadata = _parse_metadata(d.pop("metadata", UNSET))

        def _parse_node_id(data: object) -> Union[None, Unset, int]:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(Union[None, Unset, int], data)

        node_id = _parse_node_id(d.pop("node_id", UNSET))

        def _parse_score(data: object) -> Union[None, Unset, float]:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(Union[None, Unset, float], data)

        score = _parse_score(d.pop("score", UNSET))

        def _parse_vector_score(data: object) -> Union[None, Unset, float]:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(Union[None, Unset, float], data)

        vector_score = _parse_vector_score(d.pop("vector_score", UNSET))

        graph_rag_hit = cls(
            final_score=final_score,
            graph_score=graph_score,
            memory_id=memory_id,
            record_id=record_id,
            source=source,
            graph_distance=graph_distance,
            metadata=metadata,
            node_id=node_id,
            score=score,
            vector_score=vector_score,
        )

        graph_rag_hit.additional_properties = d
        return graph_rag_hit

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
