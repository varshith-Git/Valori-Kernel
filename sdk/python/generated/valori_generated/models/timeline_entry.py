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

T = TypeVar("T", bound="TimelineEntry")


@_attrs_define
class TimelineEntry:
    """A single entry in the timeline — one committed kernel event with its metadata.

    Attributes:
        event_type (str): Human-readable event kind.
        log_index (int): Sequential index within this entry's shard log (0-based).
            Used as a tie-breaker when two shards share the same `timestamp_unix`.
        shard_id (int): Shard that committed this event. Always 0 in standalone mode.
        timestamp_iso (str): ISO 8601 UTC string for `timestamp_unix`.
        timestamp_unix (int): Unix-second wall-clock timestamp when this event was committed.
        edge_id (Union[None, Unset, int]): Edge ID if this is a graph-edge event.
        node_id (Union[None, Unset, int]): Node ID if this is a graph-node event.
        record_id (Union[None, Unset, int]): Record ID if this is a record-level event.
    """

    event_type: str
    log_index: int
    shard_id: int
    timestamp_iso: str
    timestamp_unix: int
    edge_id: Union[None, Unset, int] = UNSET
    node_id: Union[None, Unset, int] = UNSET
    record_id: Union[None, Unset, int] = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        event_type = self.event_type

        log_index = self.log_index

        shard_id = self.shard_id

        timestamp_iso = self.timestamp_iso

        timestamp_unix = self.timestamp_unix

        edge_id: Union[None, Unset, int]
        if isinstance(self.edge_id, Unset):
            edge_id = UNSET
        else:
            edge_id = self.edge_id

        node_id: Union[None, Unset, int]
        if isinstance(self.node_id, Unset):
            node_id = UNSET
        else:
            node_id = self.node_id

        record_id: Union[None, Unset, int]
        if isinstance(self.record_id, Unset):
            record_id = UNSET
        else:
            record_id = self.record_id

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "event_type": event_type,
                "log_index": log_index,
                "shard_id": shard_id,
                "timestamp_iso": timestamp_iso,
                "timestamp_unix": timestamp_unix,
            }
        )
        if edge_id is not UNSET:
            field_dict["edge_id"] = edge_id
        if node_id is not UNSET:
            field_dict["node_id"] = node_id
        if record_id is not UNSET:
            field_dict["record_id"] = record_id

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)
        event_type = d.pop("event_type")

        log_index = d.pop("log_index")

        shard_id = d.pop("shard_id")

        timestamp_iso = d.pop("timestamp_iso")

        timestamp_unix = d.pop("timestamp_unix")

        def _parse_edge_id(data: object) -> Union[None, Unset, int]:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(Union[None, Unset, int], data)

        edge_id = _parse_edge_id(d.pop("edge_id", UNSET))

        def _parse_node_id(data: object) -> Union[None, Unset, int]:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(Union[None, Unset, int], data)

        node_id = _parse_node_id(d.pop("node_id", UNSET))

        def _parse_record_id(data: object) -> Union[None, Unset, int]:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(Union[None, Unset, int], data)

        record_id = _parse_record_id(d.pop("record_id", UNSET))

        timeline_entry = cls(
            event_type=event_type,
            log_index=log_index,
            shard_id=shard_id,
            timestamp_iso=timestamp_iso,
            timestamp_unix=timestamp_unix,
            edge_id=edge_id,
            node_id=node_id,
            record_id=record_id,
        )

        timeline_entry.additional_properties = d
        return timeline_entry

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
