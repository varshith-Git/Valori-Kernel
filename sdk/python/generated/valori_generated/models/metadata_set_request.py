from collections.abc import Mapping
from typing import (
    TYPE_CHECKING,
    Any,
    TypeVar,
)

from attrs import define as _attrs_define
from attrs import field as _attrs_field

if TYPE_CHECKING:
    from ..models.metadata_set_request_metadata import MetadataSetRequestMetadata


T = TypeVar("T", bound="MetadataSetRequest")


@_attrs_define
class MetadataSetRequest:
    """
    Attributes:
        metadata (MetadataSetRequestMetadata): Arbitrary caller-supplied JSON object. Valori stores it verbatim and
            never interprets it, so this is genuinely open-ended — but
            `additionalProperties` makes that explicit, so a generator emits
            `Record<string, unknown>` / `Dict[str, Any]` rather than a bare
            property-less `object` that says nothing at all.
        target_id (str):
    """

    metadata: "MetadataSetRequestMetadata"
    target_id: str
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        metadata = self.metadata.to_dict()

        target_id = self.target_id

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "metadata": metadata,
                "target_id": target_id,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        from ..models.metadata_set_request_metadata import MetadataSetRequestMetadata

        d = dict(src_dict)
        metadata = MetadataSetRequestMetadata.from_dict(d.pop("metadata"))

        target_id = d.pop("target_id")

        metadata_set_request = cls(
            metadata=metadata,
            target_id=target_id,
        )

        metadata_set_request.additional_properties = d
        return metadata_set_request

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
