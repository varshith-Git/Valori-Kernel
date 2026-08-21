from collections.abc import Mapping
from typing import (
    TYPE_CHECKING,
    Any,
    TypeVar,
    Union,
    cast,
)

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

if TYPE_CHECKING:
    from ..models.memory_upsert_vector_request_metadata_type_0 import (
        MemoryUpsertVectorRequestMetadataType0,
    )


T = TypeVar("T", bound="MemoryUpsertVectorRequest")


@_attrs_define
class MemoryUpsertVectorRequest:
    """
    Attributes:
        vector (list[float]):
        attach_to_document_node (Union[None, Unset, int]):
        collection (Union[None, Unset, str]):
        metadata (Union['MemoryUpsertVectorRequestMetadataType0', None, Unset]):
        tags (Union[None, Unset, list[str]]):
    """

    vector: list[float]
    attach_to_document_node: Union[None, Unset, int] = UNSET
    collection: Union[None, Unset, str] = UNSET
    metadata: Union["MemoryUpsertVectorRequestMetadataType0", None, Unset] = UNSET
    tags: Union[None, Unset, list[str]] = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        from ..models.memory_upsert_vector_request_metadata_type_0 import (
            MemoryUpsertVectorRequestMetadataType0,
        )

        vector = self.vector

        attach_to_document_node: Union[None, Unset, int]
        if isinstance(self.attach_to_document_node, Unset):
            attach_to_document_node = UNSET
        else:
            attach_to_document_node = self.attach_to_document_node

        collection: Union[None, Unset, str]
        if isinstance(self.collection, Unset):
            collection = UNSET
        else:
            collection = self.collection

        metadata: Union[None, Unset, dict[str, Any]]
        if isinstance(self.metadata, Unset):
            metadata = UNSET
        elif isinstance(self.metadata, MemoryUpsertVectorRequestMetadataType0):
            metadata = self.metadata.to_dict()
        else:
            metadata = self.metadata

        tags: Union[None, Unset, list[str]]
        if isinstance(self.tags, Unset):
            tags = UNSET
        elif isinstance(self.tags, list):
            tags = self.tags

        else:
            tags = self.tags

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "vector": vector,
            }
        )
        if attach_to_document_node is not UNSET:
            field_dict["attach_to_document_node"] = attach_to_document_node
        if collection is not UNSET:
            field_dict["collection"] = collection
        if metadata is not UNSET:
            field_dict["metadata"] = metadata
        if tags is not UNSET:
            field_dict["tags"] = tags

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        from ..models.memory_upsert_vector_request_metadata_type_0 import (
            MemoryUpsertVectorRequestMetadataType0,
        )

        d = dict(src_dict)
        vector = cast(list[float], d.pop("vector"))

        def _parse_attach_to_document_node(data: object) -> Union[None, Unset, int]:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(Union[None, Unset, int], data)

        attach_to_document_node = _parse_attach_to_document_node(
            d.pop("attach_to_document_node", UNSET)
        )

        def _parse_collection(data: object) -> Union[None, Unset, str]:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(Union[None, Unset, str], data)

        collection = _parse_collection(d.pop("collection", UNSET))

        def _parse_metadata(
            data: object,
        ) -> Union["MemoryUpsertVectorRequestMetadataType0", None, Unset]:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            try:
                if not isinstance(data, dict):
                    raise TypeError()
                metadata_type_0 = MemoryUpsertVectorRequestMetadataType0.from_dict(data)

                return metadata_type_0
            except:  # noqa: E722
                pass
            return cast(
                Union["MemoryUpsertVectorRequestMetadataType0", None, Unset], data
            )

        metadata = _parse_metadata(d.pop("metadata", UNSET))

        def _parse_tags(data: object) -> Union[None, Unset, list[str]]:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            try:
                if not isinstance(data, list):
                    raise TypeError()
                tags_type_0 = cast(list[str], data)

                return tags_type_0
            except:  # noqa: E722
                pass
            return cast(Union[None, Unset, list[str]], data)

        tags = _parse_tags(d.pop("tags", UNSET))

        memory_upsert_vector_request = cls(
            vector=vector,
            attach_to_document_node=attach_to_document_node,
            collection=collection,
            metadata=metadata,
            tags=tags,
        )

        memory_upsert_vector_request.additional_properties = d
        return memory_upsert_vector_request

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
