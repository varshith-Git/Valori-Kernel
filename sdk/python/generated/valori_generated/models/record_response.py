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
    from ..models.record_response_metadata_type_0 import RecordResponseMetadataType0


T = TypeVar("T", bound="RecordResponse")


@_attrs_define
class RecordResponse:
    """`GET /v1/records/{id}` — one stored record, decoded back to `f32`.

    Attributes:
        id (int):
        tag (int):
        vector (list[float]): The stored Q16.16 vector, converted back to `f32` for the wire.
            Round-tripping is lossy at the Q16.16 quantum, by design.
        metadata (Union['RecordResponseMetadataType0', None, Unset]): Whatever JSON was committed alongside the record,
            if any.
    """

    id: int
    tag: int
    vector: list[float]
    metadata: Union["RecordResponseMetadataType0", None, Unset] = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        from ..models.record_response_metadata_type_0 import RecordResponseMetadataType0

        id = self.id

        tag = self.tag

        vector = self.vector

        metadata: Union[None, Unset, dict[str, Any]]
        if isinstance(self.metadata, Unset):
            metadata = UNSET
        elif isinstance(self.metadata, RecordResponseMetadataType0):
            metadata = self.metadata.to_dict()
        else:
            metadata = self.metadata

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "id": id,
                "tag": tag,
                "vector": vector,
            }
        )
        if metadata is not UNSET:
            field_dict["metadata"] = metadata

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        from ..models.record_response_metadata_type_0 import RecordResponseMetadataType0

        d = dict(src_dict)
        id = d.pop("id")

        tag = d.pop("tag")

        vector = cast(list[float], d.pop("vector"))

        def _parse_metadata(
            data: object,
        ) -> Union["RecordResponseMetadataType0", None, Unset]:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            try:
                if not isinstance(data, dict):
                    raise TypeError()
                metadata_type_0 = RecordResponseMetadataType0.from_dict(data)

                return metadata_type_0
            except:  # noqa: E722
                pass
            return cast(Union["RecordResponseMetadataType0", None, Unset], data)

        metadata = _parse_metadata(d.pop("metadata", UNSET))

        record_response = cls(
            id=id,
            tag=tag,
            vector=vector,
            metadata=metadata,
        )

        record_response.additional_properties = d
        return record_response

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
