from collections.abc import Mapping
from typing import (
    Any,
    TypeVar,
    cast,
)

from attrs import define as _attrs_define
from attrs import field as _attrs_field

T = TypeVar("T", bound="IngestUpdateResponse")


@_attrs_define
class IngestUpdateResponse:
    """
    Attributes:
        added_count (int):
        collection (str):
        document_node_id (int):
        kept_count (int):
        new_chunk_count (int):
        ok (bool):
        record_ids (list[int]):
        removed_count (int):
        strategy_used (str):
    """

    added_count: int
    collection: str
    document_node_id: int
    kept_count: int
    new_chunk_count: int
    ok: bool
    record_ids: list[int]
    removed_count: int
    strategy_used: str
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        added_count = self.added_count

        collection = self.collection

        document_node_id = self.document_node_id

        kept_count = self.kept_count

        new_chunk_count = self.new_chunk_count

        ok = self.ok

        record_ids = self.record_ids

        removed_count = self.removed_count

        strategy_used = self.strategy_used

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "added_count": added_count,
                "collection": collection,
                "document_node_id": document_node_id,
                "kept_count": kept_count,
                "new_chunk_count": new_chunk_count,
                "ok": ok,
                "record_ids": record_ids,
                "removed_count": removed_count,
                "strategy_used": strategy_used,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)
        added_count = d.pop("added_count")

        collection = d.pop("collection")

        document_node_id = d.pop("document_node_id")

        kept_count = d.pop("kept_count")

        new_chunk_count = d.pop("new_chunk_count")

        ok = d.pop("ok")

        record_ids = cast(list[int], d.pop("record_ids"))

        removed_count = d.pop("removed_count")

        strategy_used = d.pop("strategy_used")

        ingest_update_response = cls(
            added_count=added_count,
            collection=collection,
            document_node_id=document_node_id,
            kept_count=kept_count,
            new_chunk_count=new_chunk_count,
            ok=ok,
            record_ids=record_ids,
            removed_count=removed_count,
            strategy_used=strategy_used,
        )

        ingest_update_response.additional_properties = d
        return ingest_update_response

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
