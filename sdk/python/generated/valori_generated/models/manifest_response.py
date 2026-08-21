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
    from ..models.snapshot_manifest import SnapshotManifest


T = TypeVar("T", bound="ManifestResponse")


@_attrs_define
class ManifestResponse:
    """
    Attributes:
        manifest (Union['SnapshotManifest', None, Unset]):
    """

    manifest: Union["SnapshotManifest", None, Unset] = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        from ..models.snapshot_manifest import SnapshotManifest

        manifest: Union[None, Unset, dict[str, Any]]
        if isinstance(self.manifest, Unset):
            manifest = UNSET
        elif isinstance(self.manifest, SnapshotManifest):
            manifest = self.manifest.to_dict()
        else:
            manifest = self.manifest

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update({})
        if manifest is not UNSET:
            field_dict["manifest"] = manifest

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        from ..models.snapshot_manifest import SnapshotManifest

        d = dict(src_dict)

        def _parse_manifest(data: object) -> Union["SnapshotManifest", None, Unset]:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            try:
                if not isinstance(data, dict):
                    raise TypeError()
                manifest_type_1 = SnapshotManifest.from_dict(data)

                return manifest_type_1
            except:  # noqa: E722
                pass
            return cast(Union["SnapshotManifest", None, Unset], data)

        manifest = _parse_manifest(d.pop("manifest", UNSET))

        manifest_response = cls(
            manifest=manifest,
        )

        manifest_response.additional_properties = d
        return manifest_response

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
