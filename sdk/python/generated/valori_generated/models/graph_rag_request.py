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

T = TypeVar("T", bound="GraphRagRequest")


@_attrs_define
class GraphRagRequest:
    """
    Attributes:
        query_vector (list[float]):
        collection (Union[None, Unset, str]):
        depth (Union[Unset, int]):
        final_k (Union[None, Unset, int]): Maximum returned hits. Absent = defaults to `retrieval_k` (Phase 5.4).
        graph_weight (Union[Unset, float]): Phase 5.4: β in `final_score = (1-β)×vector_rel + β×graph_rel`. Range [0,1].
        k (Union[None, Unset, int]): Legacy alias for `retrieval_k`. When `retrieval_k` is absent, `k` is used.
        max_edges (Union[None, Unset, int]): Phase 5.4: halt edge emission once this count is reached per BFS round.
        max_graph_candidates (Union[None, Unset, int]): Budget on graph-only candidates (applied before `final_k`).
            Absent = 100.
        max_nodes (Union[None, Unset, int]): Phase 5.4: halt BFS before visiting a node that would exceed this count.
        retrieval_k (Union[None, Unset, int]): How many vector candidates to use as seeds for graph expansion.
    """

    query_vector: list[float]
    collection: Union[None, Unset, str] = UNSET
    depth: Union[Unset, int] = UNSET
    final_k: Union[None, Unset, int] = UNSET
    graph_weight: Union[Unset, float] = UNSET
    k: Union[None, Unset, int] = UNSET
    max_edges: Union[None, Unset, int] = UNSET
    max_graph_candidates: Union[None, Unset, int] = UNSET
    max_nodes: Union[None, Unset, int] = UNSET
    retrieval_k: Union[None, Unset, int] = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        query_vector = self.query_vector

        collection: Union[None, Unset, str]
        if isinstance(self.collection, Unset):
            collection = UNSET
        else:
            collection = self.collection

        depth = self.depth

        final_k: Union[None, Unset, int]
        if isinstance(self.final_k, Unset):
            final_k = UNSET
        else:
            final_k = self.final_k

        graph_weight = self.graph_weight

        k: Union[None, Unset, int]
        if isinstance(self.k, Unset):
            k = UNSET
        else:
            k = self.k

        max_edges: Union[None, Unset, int]
        if isinstance(self.max_edges, Unset):
            max_edges = UNSET
        else:
            max_edges = self.max_edges

        max_graph_candidates: Union[None, Unset, int]
        if isinstance(self.max_graph_candidates, Unset):
            max_graph_candidates = UNSET
        else:
            max_graph_candidates = self.max_graph_candidates

        max_nodes: Union[None, Unset, int]
        if isinstance(self.max_nodes, Unset):
            max_nodes = UNSET
        else:
            max_nodes = self.max_nodes

        retrieval_k: Union[None, Unset, int]
        if isinstance(self.retrieval_k, Unset):
            retrieval_k = UNSET
        else:
            retrieval_k = self.retrieval_k

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "query_vector": query_vector,
            }
        )
        if collection is not UNSET:
            field_dict["collection"] = collection
        if depth is not UNSET:
            field_dict["depth"] = depth
        if final_k is not UNSET:
            field_dict["final_k"] = final_k
        if graph_weight is not UNSET:
            field_dict["graph_weight"] = graph_weight
        if k is not UNSET:
            field_dict["k"] = k
        if max_edges is not UNSET:
            field_dict["max_edges"] = max_edges
        if max_graph_candidates is not UNSET:
            field_dict["max_graph_candidates"] = max_graph_candidates
        if max_nodes is not UNSET:
            field_dict["max_nodes"] = max_nodes
        if retrieval_k is not UNSET:
            field_dict["retrieval_k"] = retrieval_k

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)
        query_vector = cast(list[float], d.pop("query_vector"))

        def _parse_collection(data: object) -> Union[None, Unset, str]:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(Union[None, Unset, str], data)

        collection = _parse_collection(d.pop("collection", UNSET))

        depth = d.pop("depth", UNSET)

        def _parse_final_k(data: object) -> Union[None, Unset, int]:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(Union[None, Unset, int], data)

        final_k = _parse_final_k(d.pop("final_k", UNSET))

        graph_weight = d.pop("graph_weight", UNSET)

        def _parse_k(data: object) -> Union[None, Unset, int]:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(Union[None, Unset, int], data)

        k = _parse_k(d.pop("k", UNSET))

        def _parse_max_edges(data: object) -> Union[None, Unset, int]:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(Union[None, Unset, int], data)

        max_edges = _parse_max_edges(d.pop("max_edges", UNSET))

        def _parse_max_graph_candidates(data: object) -> Union[None, Unset, int]:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(Union[None, Unset, int], data)

        max_graph_candidates = _parse_max_graph_candidates(
            d.pop("max_graph_candidates", UNSET)
        )

        def _parse_max_nodes(data: object) -> Union[None, Unset, int]:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(Union[None, Unset, int], data)

        max_nodes = _parse_max_nodes(d.pop("max_nodes", UNSET))

        def _parse_retrieval_k(data: object) -> Union[None, Unset, int]:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(Union[None, Unset, int], data)

        retrieval_k = _parse_retrieval_k(d.pop("retrieval_k", UNSET))

        graph_rag_request = cls(
            query_vector=query_vector,
            collection=collection,
            depth=depth,
            final_k=final_k,
            graph_weight=graph_weight,
            k=k,
            max_edges=max_edges,
            max_graph_candidates=max_graph_candidates,
            max_nodes=max_nodes,
            retrieval_k=retrieval_k,
        )

        graph_rag_request.additional_properties = d
        return graph_rag_request

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
