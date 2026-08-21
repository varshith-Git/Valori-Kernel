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

T = TypeVar("T", bound="MemoryContradictRequest")


@_attrs_define
class MemoryContradictRequest:
    """Check whether two records contradict each other (by cosine similarity
    threshold) and, if so, commit a Contradicts edge to the audit chain.

        Attributes:
            record_a (int):
            record_b (int):
            collection (Union[None, Unset, str]):
            threshold (Union[None, Unset, float]): Cosine similarity threshold above which the records are deemed to
                contradict. Default 0.85 — tuned for claim-level NLI in Q16.16 space.
    """

    record_a: int
    record_b: int
    collection: Union[None, Unset, str] = UNSET
    threshold: Union[None, Unset, float] = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        record_a = self.record_a

        record_b = self.record_b

        collection: Union[None, Unset, str]
        if isinstance(self.collection, Unset):
            collection = UNSET
        else:
            collection = self.collection

        threshold: Union[None, Unset, float]
        if isinstance(self.threshold, Unset):
            threshold = UNSET
        else:
            threshold = self.threshold

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "record_a": record_a,
                "record_b": record_b,
            }
        )
        if collection is not UNSET:
            field_dict["collection"] = collection
        if threshold is not UNSET:
            field_dict["threshold"] = threshold

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)
        record_a = d.pop("record_a")

        record_b = d.pop("record_b")

        def _parse_collection(data: object) -> Union[None, Unset, str]:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(Union[None, Unset, str], data)

        collection = _parse_collection(d.pop("collection", UNSET))

        def _parse_threshold(data: object) -> Union[None, Unset, float]:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(Union[None, Unset, float], data)

        threshold = _parse_threshold(d.pop("threshold", UNSET))

        memory_contradict_request = cls(
            record_a=record_a,
            record_b=record_b,
            collection=collection,
            threshold=threshold,
        )

        memory_contradict_request.additional_properties = d
        return memory_contradict_request

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
