from collections.abc import Mapping
from typing import (
    Any,
    TypeVar,
    cast,
)

from attrs import define as _attrs_define
from attrs import field as _attrs_field

T = TypeVar("T", bound="IngestResponse")


@_attrs_define
class IngestResponse:
    """
    Attributes:
        chunk_count (int):
        collection (str):
        document_node_id (int):
        ok (bool):
        operation_id (str): Fetch `GET /v1/operations/:id/execution` with this id for the full
            per-stage execution breakdown (Execution Explorer).
        record_ids (list[int]):
        strategy_used (str):
    """

    chunk_count: int
    collection: str
    document_node_id: int
    ok: bool
    operation_id: str
    record_ids: list[int]
    strategy_used: str
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        chunk_count = self.chunk_count

        collection = self.collection

        document_node_id = self.document_node_id

        ok = self.ok

        operation_id = self.operation_id

        record_ids = self.record_ids

        strategy_used = self.strategy_used

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "chunk_count": chunk_count,
                "collection": collection,
                "document_node_id": document_node_id,
                "ok": ok,
                "operation_id": operation_id,
                "record_ids": record_ids,
                "strategy_used": strategy_used,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)
        chunk_count = d.pop("chunk_count")

        collection = d.pop("collection")

        document_node_id = d.pop("document_node_id")

        ok = d.pop("ok")

        operation_id = d.pop("operation_id")

        record_ids = cast(list[int], d.pop("record_ids"))

        strategy_used = d.pop("strategy_used")

        ingest_response = cls(
            chunk_count=chunk_count,
            collection=collection,
            document_node_id=document_node_id,
            ok=ok,
            operation_id=operation_id,
            record_ids=record_ids,
            strategy_used=strategy_used,
        )

        ingest_response.additional_properties = d
        return ingest_response

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
