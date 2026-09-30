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

T = TypeVar("T", bound="AssertionEvidence")


@_attrs_define
class AssertionEvidence:
    """
    Attributes:
        chunk_id (Union[None, Unset, str]):
        passage_id (Union[None, Unset, str]):
        source (Union[None, Unset, str]):
        source_text_hash (Union[None, Unset, str]):
        span_end (Union[None, Unset, int]):
        span_start (Union[None, Unset, int]):
    """

    chunk_id: Union[None, Unset, str] = UNSET
    passage_id: Union[None, Unset, str] = UNSET
    source: Union[None, Unset, str] = UNSET
    source_text_hash: Union[None, Unset, str] = UNSET
    span_end: Union[None, Unset, int] = UNSET
    span_start: Union[None, Unset, int] = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        chunk_id: Union[None, Unset, str]
        if isinstance(self.chunk_id, Unset):
            chunk_id = UNSET
        else:
            chunk_id = self.chunk_id

        passage_id: Union[None, Unset, str]
        if isinstance(self.passage_id, Unset):
            passage_id = UNSET
        else:
            passage_id = self.passage_id

        source: Union[None, Unset, str]
        if isinstance(self.source, Unset):
            source = UNSET
        else:
            source = self.source

        source_text_hash: Union[None, Unset, str]
        if isinstance(self.source_text_hash, Unset):
            source_text_hash = UNSET
        else:
            source_text_hash = self.source_text_hash

        span_end: Union[None, Unset, int]
        if isinstance(self.span_end, Unset):
            span_end = UNSET
        else:
            span_end = self.span_end

        span_start: Union[None, Unset, int]
        if isinstance(self.span_start, Unset):
            span_start = UNSET
        else:
            span_start = self.span_start

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update({})
        if chunk_id is not UNSET:
            field_dict["chunk_id"] = chunk_id
        if passage_id is not UNSET:
            field_dict["passage_id"] = passage_id
        if source is not UNSET:
            field_dict["source"] = source
        if source_text_hash is not UNSET:
            field_dict["source_text_hash"] = source_text_hash
        if span_end is not UNSET:
            field_dict["span_end"] = span_end
        if span_start is not UNSET:
            field_dict["span_start"] = span_start

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)

        def _parse_chunk_id(data: object) -> Union[None, Unset, str]:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(Union[None, Unset, str], data)

        chunk_id = _parse_chunk_id(d.pop("chunk_id", UNSET))

        def _parse_passage_id(data: object) -> Union[None, Unset, str]:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(Union[None, Unset, str], data)

        passage_id = _parse_passage_id(d.pop("passage_id", UNSET))

        def _parse_source(data: object) -> Union[None, Unset, str]:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(Union[None, Unset, str], data)

        source = _parse_source(d.pop("source", UNSET))

        def _parse_source_text_hash(data: object) -> Union[None, Unset, str]:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(Union[None, Unset, str], data)

        source_text_hash = _parse_source_text_hash(d.pop("source_text_hash", UNSET))

        def _parse_span_end(data: object) -> Union[None, Unset, int]:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(Union[None, Unset, int], data)

        span_end = _parse_span_end(d.pop("span_end", UNSET))

        def _parse_span_start(data: object) -> Union[None, Unset, int]:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(Union[None, Unset, int], data)

        span_start = _parse_span_start(d.pop("span_start", UNSET))

        assertion_evidence = cls(
            chunk_id=chunk_id,
            passage_id=passage_id,
            source=source,
            source_text_hash=source_text_hash,
            span_end=span_end,
            span_start=span_start,
        )

        assertion_evidence.additional_properties = d
        return assertion_evidence

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
