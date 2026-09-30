from collections.abc import Mapping
from typing import Any, TypeVar

from attrs import define as _attrs_define
from attrs import field as _attrs_field

T = TypeVar("T", bound="GraphRagProvenanceEdge")


@_attrs_define
class GraphRagProvenanceEdge:
    """
    Attributes:
        edge_id (int):
        from_ (int):
        kind (int):
        to (int):
    """

    edge_id: int
    from_: int
    kind: int
    to: int
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        edge_id = self.edge_id

        from_ = self.from_

        kind = self.kind

        to = self.to

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "edge_id": edge_id,
                "from": from_,
                "kind": kind,
                "to": to,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)
        edge_id = d.pop("edge_id")

        from_ = d.pop("from")

        kind = d.pop("kind")

        to = d.pop("to")

        graph_rag_provenance_edge = cls(
            edge_id=edge_id,
            from_=from_,
            kind=kind,
            to=to,
        )

        graph_rag_provenance_edge.additional_properties = d
        return graph_rag_provenance_edge

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
