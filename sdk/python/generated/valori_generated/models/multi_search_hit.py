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

T = TypeVar("T", bound="MultiSearchHit")


@_attrs_define
class MultiSearchHit:
    """A single result from a multi-collection search, annotated with its source.

    Attributes:
        collection (str): The collection this record lives in.
        id (int): Record ID within the collection.
        score (float): Squared L2 distance to the query (smaller = closer).
        age_secs (Union[None, Unset, int]): Age of the record in seconds. Present only when decay is active.
        decay_factor (Union[None, Unset, float]): Phase C4.1 — applied decay factor in (0, 1]. Present only when decay
            is active.
    """

    collection: str
    id: int
    score: float
    age_secs: Union[None, Unset, int] = UNSET
    decay_factor: Union[None, Unset, float] = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        collection = self.collection

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

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "collection": collection,
                "id": id,
                "score": score,
            }
        )
        if age_secs is not UNSET:
            field_dict["age_secs"] = age_secs
        if decay_factor is not UNSET:
            field_dict["decay_factor"] = decay_factor

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)
        collection = d.pop("collection")

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

        multi_search_hit = cls(
            collection=collection,
            id=id,
            score=score,
            age_secs=age_secs,
            decay_factor=decay_factor,
        )

        multi_search_hit.additional_properties = d
        return multi_search_hit

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
