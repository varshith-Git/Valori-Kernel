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

T = TypeVar("T", bound="BatchInsertRequest")


@_attrs_define
class BatchInsertRequest:
    """
    Attributes:
        batch (list[list[float]]):
        collection (Union[None, Unset, str]):
        metadata (Union[None, Unset, list[Union[None, str]]]): Optional per-vector metadata blobs (UTF-8 JSON strings).
            If present, must be the same length as `batch`.
            Each entry is committed inside the `InsertRecord` event and is
            therefore included in the BLAKE3 audit chain.
        request_ids (Union[None, Unset, list[Union[None, str]]]): Per-item idempotency keys (32-hex strings = 16-byte
            UUIDs).
            If present, must be the same length as `batch`. A null entry means
            "no dedup key for this item". A repeated key causes that item to be
            skipped and the previously assigned ID is returned instead.
        texts (Union[None, Unset, list[Union[None, str]]]): Optional per-vector text strings for BM25 hybrid reranking.
            If present, must be the same length as `batch`. A null entry means
            no text is stored for that vector. Text is tokenised and indexed
            so that future /search calls with `rerank=true` can re-score results.
    """

    batch: list[list[float]]
    collection: Union[None, Unset, str] = UNSET
    metadata: Union[None, Unset, list[Union[None, str]]] = UNSET
    request_ids: Union[None, Unset, list[Union[None, str]]] = UNSET
    texts: Union[None, Unset, list[Union[None, str]]] = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        batch = []
        for batch_item_data in self.batch:
            batch_item = batch_item_data

            batch.append(batch_item)

        collection: Union[None, Unset, str]
        if isinstance(self.collection, Unset):
            collection = UNSET
        else:
            collection = self.collection

        metadata: Union[None, Unset, list[Union[None, str]]]
        if isinstance(self.metadata, Unset):
            metadata = UNSET
        elif isinstance(self.metadata, list):
            metadata = []
            for metadata_type_0_item_data in self.metadata:
                metadata_type_0_item: Union[None, str]
                metadata_type_0_item = metadata_type_0_item_data
                metadata.append(metadata_type_0_item)

        else:
            metadata = self.metadata

        request_ids: Union[None, Unset, list[Union[None, str]]]
        if isinstance(self.request_ids, Unset):
            request_ids = UNSET
        elif isinstance(self.request_ids, list):
            request_ids = []
            for request_ids_type_0_item_data in self.request_ids:
                request_ids_type_0_item: Union[None, str]
                request_ids_type_0_item = request_ids_type_0_item_data
                request_ids.append(request_ids_type_0_item)

        else:
            request_ids = self.request_ids

        texts: Union[None, Unset, list[Union[None, str]]]
        if isinstance(self.texts, Unset):
            texts = UNSET
        elif isinstance(self.texts, list):
            texts = []
            for texts_type_0_item_data in self.texts:
                texts_type_0_item: Union[None, str]
                texts_type_0_item = texts_type_0_item_data
                texts.append(texts_type_0_item)

        else:
            texts = self.texts

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "batch": batch,
            }
        )
        if collection is not UNSET:
            field_dict["collection"] = collection
        if metadata is not UNSET:
            field_dict["metadata"] = metadata
        if request_ids is not UNSET:
            field_dict["request_ids"] = request_ids
        if texts is not UNSET:
            field_dict["texts"] = texts

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)
        batch = []
        _batch = d.pop("batch")
        for batch_item_data in _batch:
            batch_item = cast(list[float], batch_item_data)

            batch.append(batch_item)

        def _parse_collection(data: object) -> Union[None, Unset, str]:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(Union[None, Unset, str], data)

        collection = _parse_collection(d.pop("collection", UNSET))

        def _parse_metadata(data: object) -> Union[None, Unset, list[Union[None, str]]]:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            try:
                if not isinstance(data, list):
                    raise TypeError()
                metadata_type_0 = []
                _metadata_type_0 = data
                for metadata_type_0_item_data in _metadata_type_0:

                    def _parse_metadata_type_0_item(data: object) -> Union[None, str]:
                        if data is None:
                            return data
                        return cast(Union[None, str], data)

                    metadata_type_0_item = _parse_metadata_type_0_item(
                        metadata_type_0_item_data
                    )

                    metadata_type_0.append(metadata_type_0_item)

                return metadata_type_0
            except:  # noqa: E722
                pass
            return cast(Union[None, Unset, list[Union[None, str]]], data)

        metadata = _parse_metadata(d.pop("metadata", UNSET))

        def _parse_request_ids(
            data: object,
        ) -> Union[None, Unset, list[Union[None, str]]]:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            try:
                if not isinstance(data, list):
                    raise TypeError()
                request_ids_type_0 = []
                _request_ids_type_0 = data
                for request_ids_type_0_item_data in _request_ids_type_0:

                    def _parse_request_ids_type_0_item(
                        data: object,
                    ) -> Union[None, str]:
                        if data is None:
                            return data
                        return cast(Union[None, str], data)

                    request_ids_type_0_item = _parse_request_ids_type_0_item(
                        request_ids_type_0_item_data
                    )

                    request_ids_type_0.append(request_ids_type_0_item)

                return request_ids_type_0
            except:  # noqa: E722
                pass
            return cast(Union[None, Unset, list[Union[None, str]]], data)

        request_ids = _parse_request_ids(d.pop("request_ids", UNSET))

        def _parse_texts(data: object) -> Union[None, Unset, list[Union[None, str]]]:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            try:
                if not isinstance(data, list):
                    raise TypeError()
                texts_type_0 = []
                _texts_type_0 = data
                for texts_type_0_item_data in _texts_type_0:

                    def _parse_texts_type_0_item(data: object) -> Union[None, str]:
                        if data is None:
                            return data
                        return cast(Union[None, str], data)

                    texts_type_0_item = _parse_texts_type_0_item(texts_type_0_item_data)

                    texts_type_0.append(texts_type_0_item)

                return texts_type_0
            except:  # noqa: E722
                pass
            return cast(Union[None, Unset, list[Union[None, str]]], data)

        texts = _parse_texts(d.pop("texts", UNSET))

        batch_insert_request = cls(
            batch=batch,
            collection=collection,
            metadata=metadata,
            request_ids=request_ids,
            texts=texts,
        )

        batch_insert_request.additional_properties = d
        return batch_insert_request

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
