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

T = TypeVar("T", bound="GraphRerankRequest")


@_attrs_define
class GraphRerankRequest:
    """
    Attributes:
        direction (Union[None, Unset, str]): `"outgoing"` (default) | `"incoming"` | `"both"` — case-insensitive,
            same convention as `GET /v1/graph/query`.
        max_depth (Union[Unset, int]): Max hop count from the seed set. Clamped to `query_graph`'s own
            `MAX_DEPTH` (4), never rejected.
        seed_count (Union[Unset, int]): Number of top vector hits to resolve as graph seeds. Clamped
            `[1, 10]` server-side (never rejected).
        weight (Union[Unset, float]): Multiplier weight per hop of graph distance. Clamped `[0.0, 1.0]`
            server-side. `adjusted = score * (1 + weight * distance)`.
    """

    direction: Union[None, Unset, str] = UNSET
    max_depth: Union[Unset, int] = UNSET
    seed_count: Union[Unset, int] = UNSET
    weight: Union[Unset, float] = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        direction: Union[None, Unset, str]
        if isinstance(self.direction, Unset):
            direction = UNSET
        else:
            direction = self.direction

        max_depth = self.max_depth

        seed_count = self.seed_count

        weight = self.weight

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update({})
        if direction is not UNSET:
            field_dict["direction"] = direction
        if max_depth is not UNSET:
            field_dict["max_depth"] = max_depth
        if seed_count is not UNSET:
            field_dict["seed_count"] = seed_count
        if weight is not UNSET:
            field_dict["weight"] = weight

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)

        def _parse_direction(data: object) -> Union[None, Unset, str]:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(Union[None, Unset, str], data)

        direction = _parse_direction(d.pop("direction", UNSET))

        max_depth = d.pop("max_depth", UNSET)

        seed_count = d.pop("seed_count", UNSET)

        weight = d.pop("weight", UNSET)

        graph_rerank_request = cls(
            direction=direction,
            max_depth=max_depth,
            seed_count=seed_count,
            weight=weight,
        )

        graph_rerank_request.additional_properties = d
        return graph_rerank_request

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
