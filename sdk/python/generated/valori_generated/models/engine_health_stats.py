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
    from ..models.pool_stats_schema import PoolStatsSchema


T = TypeVar("T", bound="EngineHealthStats")


@_attrs_define
class EngineHealthStats:
    """The `engine` sub-object of `GET /health`. Standalone mode only; absent in
    cluster mode, where the node has no single in-process engine to describe.

        Attributes:
            collections (int):
            edges (PoolStatsSchema): Slab occupancy for one kernel pool (records, graph nodes, graph edges).
            embed_enabled (bool):
            nodes (PoolStatsSchema): Slab occupancy for one kernel pool (records, graph nodes, graph edges).
            persistence (str): `event_log`, `wal`, `snapshot`, or `none`.
            records (PoolStatsSchema): Slab occupancy for one kernel pool (records, graph nodes, graph edges).
            shard_count (int):
            status (str):
            version (str):
            embed_provider (Union[None, Unset, str]):
            event_log_height (Union[None, Unset, int]):
            event_log_path (Union[None, Unset, str]):
            snapshot_path (Union[None, Unset, str]):
    """

    collections: int
    edges: "PoolStatsSchema"
    embed_enabled: bool
    nodes: "PoolStatsSchema"
    persistence: str
    records: "PoolStatsSchema"
    shard_count: int
    status: str
    version: str
    embed_provider: Union[None, Unset, str] = UNSET
    event_log_height: Union[None, Unset, int] = UNSET
    event_log_path: Union[None, Unset, str] = UNSET
    snapshot_path: Union[None, Unset, str] = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        collections = self.collections

        edges = self.edges.to_dict()

        embed_enabled = self.embed_enabled

        nodes = self.nodes.to_dict()

        persistence = self.persistence

        records = self.records.to_dict()

        shard_count = self.shard_count

        status = self.status

        version = self.version

        embed_provider: Union[None, Unset, str]
        if isinstance(self.embed_provider, Unset):
            embed_provider = UNSET
        else:
            embed_provider = self.embed_provider

        event_log_height: Union[None, Unset, int]
        if isinstance(self.event_log_height, Unset):
            event_log_height = UNSET
        else:
            event_log_height = self.event_log_height

        event_log_path: Union[None, Unset, str]
        if isinstance(self.event_log_path, Unset):
            event_log_path = UNSET
        else:
            event_log_path = self.event_log_path

        snapshot_path: Union[None, Unset, str]
        if isinstance(self.snapshot_path, Unset):
            snapshot_path = UNSET
        else:
            snapshot_path = self.snapshot_path

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "collections": collections,
                "edges": edges,
                "embed_enabled": embed_enabled,
                "nodes": nodes,
                "persistence": persistence,
                "records": records,
                "shard_count": shard_count,
                "status": status,
                "version": version,
            }
        )
        if embed_provider is not UNSET:
            field_dict["embed_provider"] = embed_provider
        if event_log_height is not UNSET:
            field_dict["event_log_height"] = event_log_height
        if event_log_path is not UNSET:
            field_dict["event_log_path"] = event_log_path
        if snapshot_path is not UNSET:
            field_dict["snapshot_path"] = snapshot_path

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        from ..models.pool_stats_schema import PoolStatsSchema

        d = dict(src_dict)
        collections = d.pop("collections")

        edges = PoolStatsSchema.from_dict(d.pop("edges"))

        embed_enabled = d.pop("embed_enabled")

        nodes = PoolStatsSchema.from_dict(d.pop("nodes"))

        persistence = d.pop("persistence")

        records = PoolStatsSchema.from_dict(d.pop("records"))

        shard_count = d.pop("shard_count")

        status = d.pop("status")

        version = d.pop("version")

        def _parse_embed_provider(data: object) -> Union[None, Unset, str]:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(Union[None, Unset, str], data)

        embed_provider = _parse_embed_provider(d.pop("embed_provider", UNSET))

        def _parse_event_log_height(data: object) -> Union[None, Unset, int]:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(Union[None, Unset, int], data)

        event_log_height = _parse_event_log_height(d.pop("event_log_height", UNSET))

        def _parse_event_log_path(data: object) -> Union[None, Unset, str]:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(Union[None, Unset, str], data)

        event_log_path = _parse_event_log_path(d.pop("event_log_path", UNSET))

        def _parse_snapshot_path(data: object) -> Union[None, Unset, str]:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(Union[None, Unset, str], data)

        snapshot_path = _parse_snapshot_path(d.pop("snapshot_path", UNSET))

        engine_health_stats = cls(
            collections=collections,
            edges=edges,
            embed_enabled=embed_enabled,
            nodes=nodes,
            persistence=persistence,
            records=records,
            shard_count=shard_count,
            status=status,
            version=version,
            embed_provider=embed_provider,
            event_log_height=event_log_height,
            event_log_path=event_log_path,
            snapshot_path=snapshot_path,
        )

        engine_health_stats.additional_properties = d
        return engine_health_stats

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
