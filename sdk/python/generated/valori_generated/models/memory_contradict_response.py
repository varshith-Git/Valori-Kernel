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

T = TypeVar("T", bound="MemoryContradictResponse")


@_attrs_define
class MemoryContradictResponse:
    """
    Attributes:
        contradicts (bool):
        record_a (int):
        record_b (int):
        similarity (float):
        state_hash (str):
        edge_id (Union[None, Unset, int]): Edge id of the Contradicts edge, present only when contradicts=true.
        log_index (Union[None, Unset, int]):
    """

    contradicts: bool
    record_a: int
    record_b: int
    similarity: float
    state_hash: str
    edge_id: Union[None, Unset, int] = UNSET
    log_index: Union[None, Unset, int] = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        contradicts = self.contradicts

        record_a = self.record_a

        record_b = self.record_b

        similarity = self.similarity

        state_hash = self.state_hash

        edge_id: Union[None, Unset, int]
        if isinstance(self.edge_id, Unset):
            edge_id = UNSET
        else:
            edge_id = self.edge_id

        log_index: Union[None, Unset, int]
        if isinstance(self.log_index, Unset):
            log_index = UNSET
        else:
            log_index = self.log_index

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "contradicts": contradicts,
                "record_a": record_a,
                "record_b": record_b,
                "similarity": similarity,
                "state_hash": state_hash,
            }
        )
        if edge_id is not UNSET:
            field_dict["edge_id"] = edge_id
        if log_index is not UNSET:
            field_dict["log_index"] = log_index

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)
        contradicts = d.pop("contradicts")

        record_a = d.pop("record_a")

        record_b = d.pop("record_b")

        similarity = d.pop("similarity")

        state_hash = d.pop("state_hash")

        def _parse_edge_id(data: object) -> Union[None, Unset, int]:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(Union[None, Unset, int], data)

        edge_id = _parse_edge_id(d.pop("edge_id", UNSET))

        def _parse_log_index(data: object) -> Union[None, Unset, int]:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(Union[None, Unset, int], data)

        log_index = _parse_log_index(d.pop("log_index", UNSET))

        memory_contradict_response = cls(
            contradicts=contradicts,
            record_a=record_a,
            record_b=record_b,
            similarity=similarity,
            state_hash=state_hash,
            edge_id=edge_id,
            log_index=log_index,
        )

        memory_contradict_response.additional_properties = d
        return memory_contradict_response

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
