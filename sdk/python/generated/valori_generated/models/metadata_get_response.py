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
    from ..models.metadata_get_response_metadata_type_0 import (
        MetadataGetResponseMetadataType0,
    )


T = TypeVar("T", bound="MetadataGetResponse")


@_attrs_define
class MetadataGetResponse:
    """
    Attributes:
        target_id (str):
        metadata (Union['MetadataGetResponseMetadataType0', None, Unset]):
    """

    target_id: str
    metadata: Union["MetadataGetResponseMetadataType0", None, Unset] = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        from ..models.metadata_get_response_metadata_type_0 import (
            MetadataGetResponseMetadataType0,
        )

        target_id = self.target_id

        metadata: Union[None, Unset, dict[str, Any]]
        if isinstance(self.metadata, Unset):
            metadata = UNSET
        elif isinstance(self.metadata, MetadataGetResponseMetadataType0):
            metadata = self.metadata.to_dict()
        else:
            metadata = self.metadata

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "target_id": target_id,
            }
        )
        if metadata is not UNSET:
            field_dict["metadata"] = metadata

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        from ..models.metadata_get_response_metadata_type_0 import (
            MetadataGetResponseMetadataType0,
        )

        d = dict(src_dict)
        target_id = d.pop("target_id")

        def _parse_metadata(
            data: object,
        ) -> Union["MetadataGetResponseMetadataType0", None, Unset]:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            try:
                if not isinstance(data, dict):
                    raise TypeError()
                metadata_type_0 = MetadataGetResponseMetadataType0.from_dict(data)

                return metadata_type_0
            except:  # noqa: E722
                pass
            return cast(Union["MetadataGetResponseMetadataType0", None, Unset], data)

        metadata = _parse_metadata(d.pop("metadata", UNSET))

        metadata_get_response = cls(
            target_id=target_id,
            metadata=metadata,
        )

        metadata_get_response.additional_properties = d
        return metadata_get_response

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
