from collections.abc import Mapping
from typing import (
    TYPE_CHECKING,
    Any,
    TypeVar,
)

from attrs import define as _attrs_define
from attrs import field as _attrs_field

if TYPE_CHECKING:
    from ..models.ingest_chunk import IngestChunk


T = TypeVar("T", bound="IngestDocumentResponse")


@_attrs_define
class IngestDocumentResponse:
    """
    Attributes:
        chunk_count (int): Total number of chunks produced.
        chunks (list['IngestChunk']): The chunks. Caller embeds each `text`, inserts the vector, records
            `record_id` → chunk for provenance.
        collection (str): Collection the document was targeted at.
        strategy_used (str): Strategy that was actually used (useful when `strategy="auto"`).
    """

    chunk_count: int
    chunks: list["IngestChunk"]
    collection: str
    strategy_used: str
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        chunk_count = self.chunk_count

        chunks = []
        for chunks_item_data in self.chunks:
            chunks_item = chunks_item_data.to_dict()
            chunks.append(chunks_item)

        collection = self.collection

        strategy_used = self.strategy_used

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "chunk_count": chunk_count,
                "chunks": chunks,
                "collection": collection,
                "strategy_used": strategy_used,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        from ..models.ingest_chunk import IngestChunk

        d = dict(src_dict)
        chunk_count = d.pop("chunk_count")

        chunks = []
        _chunks = d.pop("chunks")
        for chunks_item_data in _chunks:
            chunks_item = IngestChunk.from_dict(chunks_item_data)

            chunks.append(chunks_item)

        collection = d.pop("collection")

        strategy_used = d.pop("strategy_used")

        ingest_document_response = cls(
            chunk_count=chunk_count,
            chunks=chunks,
            collection=collection,
            strategy_used=strategy_used,
        )

        ingest_document_response.additional_properties = d
        return ingest_document_response

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
