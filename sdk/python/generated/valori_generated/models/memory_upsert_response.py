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

T = TypeVar("T", bound="MemoryUpsertResponse")


@_attrs_define
class MemoryUpsertResponse:
    """
    Attributes:
        chunk_node_id (int):
        document_node_id (int):
        memory_id (str):
        record_id (int):
        log_index (Union[None, Unset, int]):
    """

    chunk_node_id: int
    document_node_id: int
    memory_id: str
    record_id: int
    log_index: Union[None, Unset, int] = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        chunk_node_id = self.chunk_node_id

        document_node_id = self.document_node_id

        memory_id = self.memory_id

        record_id = self.record_id

        log_index: Union[None, Unset, int]
        if isinstance(self.log_index, Unset):
            log_index = UNSET
        else:
            log_index = self.log_index

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "chunk_node_id": chunk_node_id,
                "document_node_id": document_node_id,
                "memory_id": memory_id,
                "record_id": record_id,
            }
        )
        if log_index is not UNSET:
            field_dict["log_index"] = log_index

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)
        chunk_node_id = d.pop("chunk_node_id")

        document_node_id = d.pop("document_node_id")

        memory_id = d.pop("memory_id")

        record_id = d.pop("record_id")

        def _parse_log_index(data: object) -> Union[None, Unset, int]:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(Union[None, Unset, int], data)

        log_index = _parse_log_index(d.pop("log_index", UNSET))

        memory_upsert_response = cls(
            chunk_node_id=chunk_node_id,
            document_node_id=document_node_id,
            memory_id=memory_id,
            record_id=record_id,
            log_index=log_index,
        )

        memory_upsert_response.additional_properties = d
        return memory_upsert_response

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
