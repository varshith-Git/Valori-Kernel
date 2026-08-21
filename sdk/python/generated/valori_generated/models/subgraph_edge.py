from collections.abc import Mapping
from typing import Any, TypeVar

from attrs import define as _attrs_define
from attrs import field as _attrs_field

T = TypeVar("T", bound="SubgraphEdge")


@_attrs_define
class SubgraphEdge:
    """One edge in an expanded subgraph, as emitted by
    `valori_rag::graph::expand_subgraph`.

        Attributes:
            from_ (int): Source node id.
            id (int): Graph edge id.
            kind (int): `EdgeKind` discriminant.
            to (int): Target node id.
    """

    from_: int
    id: int
    kind: int
    to: int
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        from_ = self.from_

        id = self.id

        kind = self.kind

        to = self.to

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "from": from_,
                "id": id,
                "kind": kind,
                "to": to,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)
        from_ = d.pop("from")

        id = d.pop("id")

        kind = d.pop("kind")

        to = d.pop("to")

        subgraph_edge = cls(
            from_=from_,
            id=id,
            kind=kind,
            to=to,
        )

        subgraph_edge.additional_properties = d
        return subgraph_edge

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
