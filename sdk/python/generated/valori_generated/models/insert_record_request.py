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

T = TypeVar("T", bound="InsertRecordRequest")


@_attrs_define
class InsertRecordRequest:
    """**The** public request body for `POST /v1/records` — one model, both routers.

    Phase API-2 merged the two divergent bodies that existed before:
    standalone accepted `{values, collection, text}` and silently discarded
    everything else; cluster accepted `{values, collection, metadata, tag,
    request_id}` and silently discarded `text`. Every field below is now
    honoured on **both** paths.

        Attributes:
            values (list[float]):
            collection (Union[None, Unset, str]):
            metadata (Union[None, Unset, list[int]]): Opaque per-record metadata bytes, committed inside the `InsertRecord`
                event and therefore covered by the BLAKE3 audit chain.
            request_id (Union[None, Unset, list[int], str]):
            tag (Union[Unset, int]): Opaque user tag stored alongside the record.
            text (Union[None, Unset, str]): Optional raw text for BM25 hybrid reranking. When provided, stored
                in the reranker index alongside the vector so future searches can
                use term-frequency scoring to reorder results.
    """

    values: list[float]
    collection: Union[None, Unset, str] = UNSET
    metadata: Union[None, Unset, list[int]] = UNSET
    request_id: Union[None, Unset, list[int], str] = UNSET
    tag: Union[Unset, int] = UNSET
    text: Union[None, Unset, str] = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        values = self.values

        collection: Union[None, Unset, str]
        if isinstance(self.collection, Unset):
            collection = UNSET
        else:
            collection = self.collection

        metadata: Union[None, Unset, list[int]]
        if isinstance(self.metadata, Unset):
            metadata = UNSET
        elif isinstance(self.metadata, list):
            metadata = self.metadata

        else:
            metadata = self.metadata

        request_id: Union[None, Unset, list[int], str]
        if isinstance(self.request_id, Unset):
            request_id = UNSET
        elif isinstance(self.request_id, list):
            request_id = self.request_id

        else:
            request_id = self.request_id

        tag = self.tag

        text: Union[None, Unset, str]
        if isinstance(self.text, Unset):
            text = UNSET
        else:
            text = self.text

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "values": values,
            }
        )
        if collection is not UNSET:
            field_dict["collection"] = collection
        if metadata is not UNSET:
            field_dict["metadata"] = metadata
        if request_id is not UNSET:
            field_dict["request_id"] = request_id
        if tag is not UNSET:
            field_dict["tag"] = tag
        if text is not UNSET:
            field_dict["text"] = text

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)
        values = cast(list[float], d.pop("values"))

        def _parse_collection(data: object) -> Union[None, Unset, str]:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(Union[None, Unset, str], data)

        collection = _parse_collection(d.pop("collection", UNSET))

        def _parse_metadata(data: object) -> Union[None, Unset, list[int]]:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            try:
                if not isinstance(data, list):
                    raise TypeError()
                metadata_type_0 = cast(list[int], data)

                return metadata_type_0
            except:  # noqa: E722
                pass
            return cast(Union[None, Unset, list[int]], data)

        metadata = _parse_metadata(d.pop("metadata", UNSET))

        def _parse_request_id(data: object) -> Union[None, Unset, list[int], str]:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            try:
                if not isinstance(data, list):
                    raise TypeError()
                componentsschemas_request_id_type_1 = cast(list[int], data)

                return componentsschemas_request_id_type_1
            except:  # noqa: E722
                pass
            return cast(Union[None, Unset, list[int], str], data)

        request_id = _parse_request_id(d.pop("request_id", UNSET))

        tag = d.pop("tag", UNSET)

        def _parse_text(data: object) -> Union[None, Unset, str]:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(Union[None, Unset, str], data)

        text = _parse_text(d.pop("text", UNSET))

        insert_record_request = cls(
            values=values,
            collection=collection,
            metadata=metadata,
            request_id=request_id,
            tag=tag,
            text=text,
        )

        insert_record_request.additional_properties = d
        return insert_record_request

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
