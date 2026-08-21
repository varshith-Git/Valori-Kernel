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
    from ..models.graph_rag_hit import GraphRagHit
    from ..models.subgraph_response import SubgraphResponse


T = TypeVar("T", bound="GraphRagResponse")


@_attrs_define
class GraphRagResponse:
    """`POST /v1/graphrag` — K nearest vectors plus the connected subgraph around
    them, read from one consistent kernel snapshot.

        Attributes:
            hits (list['GraphRagHit']): Blended vector + graph hits, best first.
            seed_nodes (list[int]): Graph node ids the vector hits seeded the expansion from.
            subgraph (SubgraphResponse): `GET /v1/graph/subgraph` — a BFS expansion around one root node.
    """

    hits: list["GraphRagHit"]
    seed_nodes: list[int]
    subgraph: "SubgraphResponse"
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        hits = []
        for hits_item_data in self.hits:
            hits_item = hits_item_data.to_dict()
            hits.append(hits_item)

        seed_nodes = self.seed_nodes

        subgraph = self.subgraph.to_dict()

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "hits": hits,
                "seed_nodes": seed_nodes,
                "subgraph": subgraph,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        from ..models.graph_rag_hit import GraphRagHit
        from ..models.subgraph_response import SubgraphResponse

        d = dict(src_dict)
        hits = []
        _hits = d.pop("hits")
        for hits_item_data in _hits:
            hits_item = GraphRagHit.from_dict(hits_item_data)

            hits.append(hits_item)

        seed_nodes = cast(list[int], d.pop("seed_nodes"))

        subgraph = SubgraphResponse.from_dict(d.pop("subgraph"))

        graph_rag_response = cls(
            hits=hits,
            seed_nodes=seed_nodes,
            subgraph=subgraph,
        )

        graph_rag_response.additional_properties = d
        return graph_rag_response

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
