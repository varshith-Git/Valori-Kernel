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
    from ..models.memory_consolidate_request_metadata_type_0 import (
        MemoryConsolidateRequestMetadataType0,
    )


T = TypeVar("T", bound="MemoryConsolidateRequest")


@_attrs_define
class MemoryConsolidateRequest:
    """Replace an existing memory record with a new vector, committing a
    SoftDeleteRecord + AutoInsertRecord + AutoCreateEdge(Supersedes) to the
    BLAKE3 audit chain in one logical operation.

        Attributes:
            new_vector (list[float]): New vector that replaces the old memory.
            old_record_id (int): Record id of the memory being replaced.
            collection (Union[None, Unset, str]):
            metadata (Union['MemoryConsolidateRequestMetadataType0', None, Unset]):
    """

    new_vector: list[float]
    old_record_id: int
    collection: Union[None, Unset, str] = UNSET
    metadata: Union["MemoryConsolidateRequestMetadataType0", None, Unset] = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        from ..models.memory_consolidate_request_metadata_type_0 import (
            MemoryConsolidateRequestMetadataType0,
        )

        new_vector = self.new_vector

        old_record_id = self.old_record_id

        collection: Union[None, Unset, str]
        if isinstance(self.collection, Unset):
            collection = UNSET
        else:
            collection = self.collection

        metadata: Union[None, Unset, dict[str, Any]]
        if isinstance(self.metadata, Unset):
            metadata = UNSET
        elif isinstance(self.metadata, MemoryConsolidateRequestMetadataType0):
            metadata = self.metadata.to_dict()
        else:
            metadata = self.metadata

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "new_vector": new_vector,
                "old_record_id": old_record_id,
            }
        )
        if collection is not UNSET:
            field_dict["collection"] = collection
        if metadata is not UNSET:
            field_dict["metadata"] = metadata

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        from ..models.memory_consolidate_request_metadata_type_0 import (
            MemoryConsolidateRequestMetadataType0,
        )

        d = dict(src_dict)
        new_vector = cast(list[float], d.pop("new_vector"))

        old_record_id = d.pop("old_record_id")

        def _parse_collection(data: object) -> Union[None, Unset, str]:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(Union[None, Unset, str], data)

        collection = _parse_collection(d.pop("collection", UNSET))

        def _parse_metadata(
            data: object,
        ) -> Union["MemoryConsolidateRequestMetadataType0", None, Unset]:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            try:
                if not isinstance(data, dict):
                    raise TypeError()
                metadata_type_0 = MemoryConsolidateRequestMetadataType0.from_dict(data)

                return metadata_type_0
            except:  # noqa: E722
                pass
            return cast(
                Union["MemoryConsolidateRequestMetadataType0", None, Unset], data
            )

        metadata = _parse_metadata(d.pop("metadata", UNSET))

        memory_consolidate_request = cls(
            new_vector=new_vector,
            old_record_id=old_record_id,
            collection=collection,
            metadata=metadata,
        )

        memory_consolidate_request.additional_properties = d
        return memory_consolidate_request

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
