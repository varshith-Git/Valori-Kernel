from collections.abc import Mapping
from typing import Any, TypeVar

from attrs import define as _attrs_define
from attrs import field as _attrs_field

T = TypeVar("T", bound="SnapshotEntry")


@_attrs_define
class SnapshotEntry:
    """
    Attributes:
        epoch_secs (int): Unix epoch seconds extracted from the key name — used for sorting.
        key (str): Full object key (e.g. `"prefix/snapshots/00000001750000000_abc12345.snap"`).
        size_bytes (int): Snapshot size in bytes.
        state_hash (str): Hex BLAKE3 state hash recorded alongside the snapshot.
    """

    epoch_secs: int
    key: str
    size_bytes: int
    state_hash: str
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        epoch_secs = self.epoch_secs

        key = self.key

        size_bytes = self.size_bytes

        state_hash = self.state_hash

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "epoch_secs": epoch_secs,
                "key": key,
                "size_bytes": size_bytes,
                "state_hash": state_hash,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)
        epoch_secs = d.pop("epoch_secs")

        key = d.pop("key")

        size_bytes = d.pop("size_bytes")

        state_hash = d.pop("state_hash")

        snapshot_entry = cls(
            epoch_secs=epoch_secs,
            key=key,
            size_bytes=size_bytes,
            state_hash=state_hash,
        )

        snapshot_entry.additional_properties = d
        return snapshot_entry

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
