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

T = TypeVar("T", bound="IngestUpdateRequest")


@_attrs_define
class IngestUpdateRequest:
    """
    Attributes:
        document_node_id (int):
        text (str):
        chunk_overlap (Union[None, Unset, int]):
        chunk_size (Union[None, Unset, int]):
        collection (Union[None, Unset, str]):
        source (Union[None, Unset, str]):
        strategy (Union[None, Unset, str]):
    """

    document_node_id: int
    text: str
    chunk_overlap: Union[None, Unset, int] = UNSET
    chunk_size: Union[None, Unset, int] = UNSET
    collection: Union[None, Unset, str] = UNSET
    source: Union[None, Unset, str] = UNSET
    strategy: Union[None, Unset, str] = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        document_node_id = self.document_node_id

        text = self.text

        chunk_overlap: Union[None, Unset, int]
        if isinstance(self.chunk_overlap, Unset):
            chunk_overlap = UNSET
        else:
            chunk_overlap = self.chunk_overlap

        chunk_size: Union[None, Unset, int]
        if isinstance(self.chunk_size, Unset):
            chunk_size = UNSET
        else:
            chunk_size = self.chunk_size

        collection: Union[None, Unset, str]
        if isinstance(self.collection, Unset):
            collection = UNSET
        else:
            collection = self.collection

        source: Union[None, Unset, str]
        if isinstance(self.source, Unset):
            source = UNSET
        else:
            source = self.source

        strategy: Union[None, Unset, str]
        if isinstance(self.strategy, Unset):
            strategy = UNSET
        else:
            strategy = self.strategy

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "document_node_id": document_node_id,
                "text": text,
            }
        )
        if chunk_overlap is not UNSET:
            field_dict["chunk_overlap"] = chunk_overlap
        if chunk_size is not UNSET:
            field_dict["chunk_size"] = chunk_size
        if collection is not UNSET:
            field_dict["collection"] = collection
        if source is not UNSET:
            field_dict["source"] = source
        if strategy is not UNSET:
            field_dict["strategy"] = strategy

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)
        document_node_id = d.pop("document_node_id")

        text = d.pop("text")

        def _parse_chunk_overlap(data: object) -> Union[None, Unset, int]:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(Union[None, Unset, int], data)

        chunk_overlap = _parse_chunk_overlap(d.pop("chunk_overlap", UNSET))

        def _parse_chunk_size(data: object) -> Union[None, Unset, int]:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(Union[None, Unset, int], data)

        chunk_size = _parse_chunk_size(d.pop("chunk_size", UNSET))

        def _parse_collection(data: object) -> Union[None, Unset, str]:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(Union[None, Unset, str], data)

        collection = _parse_collection(d.pop("collection", UNSET))

        def _parse_source(data: object) -> Union[None, Unset, str]:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(Union[None, Unset, str], data)

        source = _parse_source(d.pop("source", UNSET))

        def _parse_strategy(data: object) -> Union[None, Unset, str]:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(Union[None, Unset, str], data)

        strategy = _parse_strategy(d.pop("strategy", UNSET))

        ingest_update_request = cls(
            document_node_id=document_node_id,
            text=text,
            chunk_overlap=chunk_overlap,
            chunk_size=chunk_size,
            collection=collection,
            source=source,
            strategy=strategy,
        )

        ingest_update_request.additional_properties = d
        return ingest_update_request

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
