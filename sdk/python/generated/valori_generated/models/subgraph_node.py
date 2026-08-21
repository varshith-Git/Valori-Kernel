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

T = TypeVar("T", bound="SubgraphNode")


@_attrs_define
class SubgraphNode:
    """One node in an expanded subgraph.

    Phase API-3.3: `SubgraphResponse.nodes` was `Vec<Object>` — an array of
    property-less objects, which is `object[]` in TypeScript. The producer,
    `valori_rag::graph::expand_subgraph`, emits a fixed three-key object; this
    records it. Note the keys are `id`/`record`, not the `node_id`/`record_id`
    that [`NodeInfo`] uses — the two shapes are genuinely different and must
    not be conflated.

        Attributes:
            id (int): Graph node id.
            kind (int): `NodeKind` discriminant.
            record (Union[None, Unset, int]): The record this node represents, when it represents one.
    """

    id: int
    kind: int
    record: Union[None, Unset, int] = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        id = self.id

        kind = self.kind

        record: Union[None, Unset, int]
        if isinstance(self.record, Unset):
            record = UNSET
        else:
            record = self.record

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "id": id,
                "kind": kind,
            }
        )
        if record is not UNSET:
            field_dict["record"] = record

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)
        id = d.pop("id")

        kind = d.pop("kind")

        def _parse_record(data: object) -> Union[None, Unset, int]:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(Union[None, Unset, int], data)

        record = _parse_record(d.pop("record", UNSET))

        subgraph_node = cls(
            id=id,
            kind=kind,
            record=record,
        )

        subgraph_node.additional_properties = d
        return subgraph_node

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
