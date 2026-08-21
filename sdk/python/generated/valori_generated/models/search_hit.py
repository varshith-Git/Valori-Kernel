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

T = TypeVar("T", bound="SearchHit")


@_attrs_define
class SearchHit:
    """
    Attributes:
        id (int):
        score (float):
        age_secs (Union[None, Unset, int]): Age of the record in seconds at query time. Present only when decay is
            active and the record's creation time is known.
        decay_factor (Union[None, Unset, float]): Phase C4.1 — applied decay factor in (0, 1]. Present only when decay
            is
            active. `score` stays the true (undecayed) L2 distance for honesty;
            ranking reflects `score / decay_factor`.
        graph_distance (Union[None, Unset, int]): G1.4.1 — hop distance to the nearest `graph_rerank` seed. Present
            only when `graph_rerank` was requested. `None` within that means
            the candidate has no graph node, or is unreachable within
            `max_depth` — never causes a candidate to be dropped.
    """

    id: int
    score: float
    age_secs: Union[None, Unset, int] = UNSET
    decay_factor: Union[None, Unset, float] = UNSET
    graph_distance: Union[None, Unset, int] = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        id = self.id

        score = self.score

        age_secs: Union[None, Unset, int]
        if isinstance(self.age_secs, Unset):
            age_secs = UNSET
        else:
            age_secs = self.age_secs

        decay_factor: Union[None, Unset, float]
        if isinstance(self.decay_factor, Unset):
            decay_factor = UNSET
        else:
            decay_factor = self.decay_factor

        graph_distance: Union[None, Unset, int]
        if isinstance(self.graph_distance, Unset):
            graph_distance = UNSET
        else:
            graph_distance = self.graph_distance

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "id": id,
                "score": score,
            }
        )
        if age_secs is not UNSET:
            field_dict["age_secs"] = age_secs
        if decay_factor is not UNSET:
            field_dict["decay_factor"] = decay_factor
        if graph_distance is not UNSET:
            field_dict["graph_distance"] = graph_distance

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)
        id = d.pop("id")

        score = d.pop("score")

        def _parse_age_secs(data: object) -> Union[None, Unset, int]:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(Union[None, Unset, int], data)

        age_secs = _parse_age_secs(d.pop("age_secs", UNSET))

        def _parse_decay_factor(data: object) -> Union[None, Unset, float]:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(Union[None, Unset, float], data)

        decay_factor = _parse_decay_factor(d.pop("decay_factor", UNSET))

        def _parse_graph_distance(data: object) -> Union[None, Unset, int]:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(Union[None, Unset, int], data)

        graph_distance = _parse_graph_distance(d.pop("graph_distance", UNSET))

        search_hit = cls(
            id=id,
            score=score,
            age_secs=age_secs,
            decay_factor=decay_factor,
            graph_distance=graph_distance,
        )

        search_hit.additional_properties = d
        return search_hit

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
