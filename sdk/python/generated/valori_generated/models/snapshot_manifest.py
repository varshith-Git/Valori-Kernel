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
    from ..models.snapshot_entry import SnapshotEntry
    from ..models.wal_entry import WalEntry


T = TypeVar("T", bound="SnapshotManifest")


@_attrs_define
class SnapshotManifest:
    """`manifest.json` — the entry point for disaster recovery. Written
    alongside every snapshot upload (see [`ObjectStoreBackend::
    upload_snapshot_and_update_manifest`]), it names the ONE snapshot that
    is current (out of however many timestamped `.snap` objects exist under
    `snapshots/` — old ones aren't deleted until `prune_snapshots` runs) plus
    the WAL segments archived since, so a restore tool has a single object
    to fetch instead of listing-and-sorting `snapshots/`/`wal/` and hoping
    the newest filename really is the right one.

        Attributes:
            node_version (str): `CARGO_PKG_VERSION` of whatever wrote this manifest (valori-node,
                normally) — lets a restore tool detect "this snapshot was written by
                an older/newer node than the one about to restore it."
            schema_version (int):
            updated_at (int): Unix epoch seconds when this manifest was last written.
            wal_segments (list['WalEntry']):
            current_snapshot (Union['SnapshotEntry', None, Unset]):
    """

    node_version: str
    schema_version: int
    updated_at: int
    wal_segments: list["WalEntry"]
    current_snapshot: Union["SnapshotEntry", None, Unset] = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        from ..models.snapshot_entry import SnapshotEntry

        node_version = self.node_version

        schema_version = self.schema_version

        updated_at = self.updated_at

        wal_segments = []
        for wal_segments_item_data in self.wal_segments:
            wal_segments_item = wal_segments_item_data.to_dict()
            wal_segments.append(wal_segments_item)

        current_snapshot: Union[None, Unset, dict[str, Any]]
        if isinstance(self.current_snapshot, Unset):
            current_snapshot = UNSET
        elif isinstance(self.current_snapshot, SnapshotEntry):
            current_snapshot = self.current_snapshot.to_dict()
        else:
            current_snapshot = self.current_snapshot

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "node_version": node_version,
                "schema_version": schema_version,
                "updated_at": updated_at,
                "wal_segments": wal_segments,
            }
        )
        if current_snapshot is not UNSET:
            field_dict["current_snapshot"] = current_snapshot

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        from ..models.snapshot_entry import SnapshotEntry
        from ..models.wal_entry import WalEntry

        d = dict(src_dict)
        node_version = d.pop("node_version")

        schema_version = d.pop("schema_version")

        updated_at = d.pop("updated_at")

        wal_segments = []
        _wal_segments = d.pop("wal_segments")
        for wal_segments_item_data in _wal_segments:
            wal_segments_item = WalEntry.from_dict(wal_segments_item_data)

            wal_segments.append(wal_segments_item)

        def _parse_current_snapshot(
            data: object,
        ) -> Union["SnapshotEntry", None, Unset]:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            try:
                if not isinstance(data, dict):
                    raise TypeError()
                current_snapshot_type_1 = SnapshotEntry.from_dict(data)

                return current_snapshot_type_1
            except:  # noqa: E722
                pass
            return cast(Union["SnapshotEntry", None, Unset], data)

        current_snapshot = _parse_current_snapshot(d.pop("current_snapshot", UNSET))

        snapshot_manifest = cls(
            node_version=node_version,
            schema_version=schema_version,
            updated_at=updated_at,
            wal_segments=wal_segments,
            current_snapshot=current_snapshot,
        )

        snapshot_manifest.additional_properties = d
        return snapshot_manifest

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
