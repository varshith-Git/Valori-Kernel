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
    from ..models.cluster_health_stats import ClusterHealthStats
    from ..models.engine_health_stats import EngineHealthStats
    from ..models.pool_stats_schema import PoolStatsSchema


T = TypeVar("T", bound="HealthResponse")


@_attrs_define
class HealthResponse:
    """
    Attributes:
        mode (str):
        shard_count (int):
        status (str):
        version (str):
        cluster (Union['ClusterHealthStats', None, Unset]):
        collections (Union[None, Unset, int]):
        dim (Union[None, Unset, int]): The vector dimension the cluster has locked to, or the configured
            dimension when nothing has been inserted yet. Cluster mode only.
        edges (Union['PoolStatsSchema', None, Unset]):
        embed_enabled (Union[None, Unset, bool]):
        embed_provider (Union[None, Unset, str]):
        engine (Union['EngineHealthStats', None, Unset]):
        event_log_height (Union[None, Unset, int]):
        leader (Union[None, Unset, int]): Node id of the leader this node currently sees. Cluster mode only.
        leader_id (Union[None, Unset, int]):
        members (Union[None, Unset, int]):
        node_id (Union[None, Unset, int]):
        nodes (Union['PoolStatsSchema', None, Unset]):
        persistence (Union[None, Unset, str]):
        raft_state (Union[None, Unset, str]):
        records (Union['PoolStatsSchema', None, Unset]):
        role (Union[None, Unset, str]):
        state_hash (Union[None, Unset, str]):
        term (Union[None, Unset, int]):
    """

    mode: str
    shard_count: int
    status: str
    version: str
    cluster: Union["ClusterHealthStats", None, Unset] = UNSET
    collections: Union[None, Unset, int] = UNSET
    dim: Union[None, Unset, int] = UNSET
    edges: Union["PoolStatsSchema", None, Unset] = UNSET
    embed_enabled: Union[None, Unset, bool] = UNSET
    embed_provider: Union[None, Unset, str] = UNSET
    engine: Union["EngineHealthStats", None, Unset] = UNSET
    event_log_height: Union[None, Unset, int] = UNSET
    leader: Union[None, Unset, int] = UNSET
    leader_id: Union[None, Unset, int] = UNSET
    members: Union[None, Unset, int] = UNSET
    node_id: Union[None, Unset, int] = UNSET
    nodes: Union["PoolStatsSchema", None, Unset] = UNSET
    persistence: Union[None, Unset, str] = UNSET
    raft_state: Union[None, Unset, str] = UNSET
    records: Union["PoolStatsSchema", None, Unset] = UNSET
    role: Union[None, Unset, str] = UNSET
    state_hash: Union[None, Unset, str] = UNSET
    term: Union[None, Unset, int] = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        from ..models.cluster_health_stats import ClusterHealthStats
        from ..models.engine_health_stats import EngineHealthStats
        from ..models.pool_stats_schema import PoolStatsSchema

        mode = self.mode

        shard_count = self.shard_count

        status = self.status

        version = self.version

        cluster: Union[None, Unset, dict[str, Any]]
        if isinstance(self.cluster, Unset):
            cluster = UNSET
        elif isinstance(self.cluster, ClusterHealthStats):
            cluster = self.cluster.to_dict()
        else:
            cluster = self.cluster

        collections: Union[None, Unset, int]
        if isinstance(self.collections, Unset):
            collections = UNSET
        else:
            collections = self.collections

        dim: Union[None, Unset, int]
        if isinstance(self.dim, Unset):
            dim = UNSET
        else:
            dim = self.dim

        edges: Union[None, Unset, dict[str, Any]]
        if isinstance(self.edges, Unset):
            edges = UNSET
        elif isinstance(self.edges, PoolStatsSchema):
            edges = self.edges.to_dict()
        else:
            edges = self.edges

        embed_enabled: Union[None, Unset, bool]
        if isinstance(self.embed_enabled, Unset):
            embed_enabled = UNSET
        else:
            embed_enabled = self.embed_enabled

        embed_provider: Union[None, Unset, str]
        if isinstance(self.embed_provider, Unset):
            embed_provider = UNSET
        else:
            embed_provider = self.embed_provider

        engine: Union[None, Unset, dict[str, Any]]
        if isinstance(self.engine, Unset):
            engine = UNSET
        elif isinstance(self.engine, EngineHealthStats):
            engine = self.engine.to_dict()
        else:
            engine = self.engine

        event_log_height: Union[None, Unset, int]
        if isinstance(self.event_log_height, Unset):
            event_log_height = UNSET
        else:
            event_log_height = self.event_log_height

        leader: Union[None, Unset, int]
        if isinstance(self.leader, Unset):
            leader = UNSET
        else:
            leader = self.leader

        leader_id: Union[None, Unset, int]
        if isinstance(self.leader_id, Unset):
            leader_id = UNSET
        else:
            leader_id = self.leader_id

        members: Union[None, Unset, int]
        if isinstance(self.members, Unset):
            members = UNSET
        else:
            members = self.members

        node_id: Union[None, Unset, int]
        if isinstance(self.node_id, Unset):
            node_id = UNSET
        else:
            node_id = self.node_id

        nodes: Union[None, Unset, dict[str, Any]]
        if isinstance(self.nodes, Unset):
            nodes = UNSET
        elif isinstance(self.nodes, PoolStatsSchema):
            nodes = self.nodes.to_dict()
        else:
            nodes = self.nodes

        persistence: Union[None, Unset, str]
        if isinstance(self.persistence, Unset):
            persistence = UNSET
        else:
            persistence = self.persistence

        raft_state: Union[None, Unset, str]
        if isinstance(self.raft_state, Unset):
            raft_state = UNSET
        else:
            raft_state = self.raft_state

        records: Union[None, Unset, dict[str, Any]]
        if isinstance(self.records, Unset):
            records = UNSET
        elif isinstance(self.records, PoolStatsSchema):
            records = self.records.to_dict()
        else:
            records = self.records

        role: Union[None, Unset, str]
        if isinstance(self.role, Unset):
            role = UNSET
        else:
            role = self.role

        state_hash: Union[None, Unset, str]
        if isinstance(self.state_hash, Unset):
            state_hash = UNSET
        else:
            state_hash = self.state_hash

        term: Union[None, Unset, int]
        if isinstance(self.term, Unset):
            term = UNSET
        else:
            term = self.term

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "mode": mode,
                "shard_count": shard_count,
                "status": status,
                "version": version,
            }
        )
        if cluster is not UNSET:
            field_dict["cluster"] = cluster
        if collections is not UNSET:
            field_dict["collections"] = collections
        if dim is not UNSET:
            field_dict["dim"] = dim
        if edges is not UNSET:
            field_dict["edges"] = edges
        if embed_enabled is not UNSET:
            field_dict["embed_enabled"] = embed_enabled
        if embed_provider is not UNSET:
            field_dict["embed_provider"] = embed_provider
        if engine is not UNSET:
            field_dict["engine"] = engine
        if event_log_height is not UNSET:
            field_dict["event_log_height"] = event_log_height
        if leader is not UNSET:
            field_dict["leader"] = leader
        if leader_id is not UNSET:
            field_dict["leader_id"] = leader_id
        if members is not UNSET:
            field_dict["members"] = members
        if node_id is not UNSET:
            field_dict["node_id"] = node_id
        if nodes is not UNSET:
            field_dict["nodes"] = nodes
        if persistence is not UNSET:
            field_dict["persistence"] = persistence
        if raft_state is not UNSET:
            field_dict["raft_state"] = raft_state
        if records is not UNSET:
            field_dict["records"] = records
        if role is not UNSET:
            field_dict["role"] = role
        if state_hash is not UNSET:
            field_dict["state_hash"] = state_hash
        if term is not UNSET:
            field_dict["term"] = term

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        from ..models.cluster_health_stats import ClusterHealthStats
        from ..models.engine_health_stats import EngineHealthStats
        from ..models.pool_stats_schema import PoolStatsSchema

        d = dict(src_dict)
        mode = d.pop("mode")

        shard_count = d.pop("shard_count")

        status = d.pop("status")

        version = d.pop("version")

        def _parse_cluster(data: object) -> Union["ClusterHealthStats", None, Unset]:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            try:
                if not isinstance(data, dict):
                    raise TypeError()
                cluster_type_1 = ClusterHealthStats.from_dict(data)

                return cluster_type_1
            except:  # noqa: E722
                pass
            return cast(Union["ClusterHealthStats", None, Unset], data)

        cluster = _parse_cluster(d.pop("cluster", UNSET))

        def _parse_collections(data: object) -> Union[None, Unset, int]:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(Union[None, Unset, int], data)

        collections = _parse_collections(d.pop("collections", UNSET))

        def _parse_dim(data: object) -> Union[None, Unset, int]:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(Union[None, Unset, int], data)

        dim = _parse_dim(d.pop("dim", UNSET))

        def _parse_edges(data: object) -> Union["PoolStatsSchema", None, Unset]:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            try:
                if not isinstance(data, dict):
                    raise TypeError()
                edges_type_1 = PoolStatsSchema.from_dict(data)

                return edges_type_1
            except:  # noqa: E722
                pass
            return cast(Union["PoolStatsSchema", None, Unset], data)

        edges = _parse_edges(d.pop("edges", UNSET))

        def _parse_embed_enabled(data: object) -> Union[None, Unset, bool]:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(Union[None, Unset, bool], data)

        embed_enabled = _parse_embed_enabled(d.pop("embed_enabled", UNSET))

        def _parse_embed_provider(data: object) -> Union[None, Unset, str]:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(Union[None, Unset, str], data)

        embed_provider = _parse_embed_provider(d.pop("embed_provider", UNSET))

        def _parse_engine(data: object) -> Union["EngineHealthStats", None, Unset]:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            try:
                if not isinstance(data, dict):
                    raise TypeError()
                engine_type_1 = EngineHealthStats.from_dict(data)

                return engine_type_1
            except:  # noqa: E722
                pass
            return cast(Union["EngineHealthStats", None, Unset], data)

        engine = _parse_engine(d.pop("engine", UNSET))

        def _parse_event_log_height(data: object) -> Union[None, Unset, int]:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(Union[None, Unset, int], data)

        event_log_height = _parse_event_log_height(d.pop("event_log_height", UNSET))

        def _parse_leader(data: object) -> Union[None, Unset, int]:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(Union[None, Unset, int], data)

        leader = _parse_leader(d.pop("leader", UNSET))

        def _parse_leader_id(data: object) -> Union[None, Unset, int]:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(Union[None, Unset, int], data)

        leader_id = _parse_leader_id(d.pop("leader_id", UNSET))

        def _parse_members(data: object) -> Union[None, Unset, int]:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(Union[None, Unset, int], data)

        members = _parse_members(d.pop("members", UNSET))

        def _parse_node_id(data: object) -> Union[None, Unset, int]:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(Union[None, Unset, int], data)

        node_id = _parse_node_id(d.pop("node_id", UNSET))

        def _parse_nodes(data: object) -> Union["PoolStatsSchema", None, Unset]:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            try:
                if not isinstance(data, dict):
                    raise TypeError()
                nodes_type_1 = PoolStatsSchema.from_dict(data)

                return nodes_type_1
            except:  # noqa: E722
                pass
            return cast(Union["PoolStatsSchema", None, Unset], data)

        nodes = _parse_nodes(d.pop("nodes", UNSET))

        def _parse_persistence(data: object) -> Union[None, Unset, str]:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(Union[None, Unset, str], data)

        persistence = _parse_persistence(d.pop("persistence", UNSET))

        def _parse_raft_state(data: object) -> Union[None, Unset, str]:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(Union[None, Unset, str], data)

        raft_state = _parse_raft_state(d.pop("raft_state", UNSET))

        def _parse_records(data: object) -> Union["PoolStatsSchema", None, Unset]:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            try:
                if not isinstance(data, dict):
                    raise TypeError()
                records_type_1 = PoolStatsSchema.from_dict(data)

                return records_type_1
            except:  # noqa: E722
                pass
            return cast(Union["PoolStatsSchema", None, Unset], data)

        records = _parse_records(d.pop("records", UNSET))

        def _parse_role(data: object) -> Union[None, Unset, str]:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(Union[None, Unset, str], data)

        role = _parse_role(d.pop("role", UNSET))

        def _parse_state_hash(data: object) -> Union[None, Unset, str]:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(Union[None, Unset, str], data)

        state_hash = _parse_state_hash(d.pop("state_hash", UNSET))

        def _parse_term(data: object) -> Union[None, Unset, int]:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(Union[None, Unset, int], data)

        term = _parse_term(d.pop("term", UNSET))

        health_response = cls(
            mode=mode,
            shard_count=shard_count,
            status=status,
            version=version,
            cluster=cluster,
            collections=collections,
            dim=dim,
            edges=edges,
            embed_enabled=embed_enabled,
            embed_provider=embed_provider,
            engine=engine,
            event_log_height=event_log_height,
            leader=leader,
            leader_id=leader_id,
            members=members,
            node_id=node_id,
            nodes=nodes,
            persistence=persistence,
            raft_state=raft_state,
            records=records,
            role=role,
            state_hash=state_hash,
            term=term,
        )

        health_response.additional_properties = d
        return health_response

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
