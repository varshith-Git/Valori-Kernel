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

T = TypeVar("T", bound="ClusterHealthStats")


@_attrs_define
class ClusterHealthStats:
    """The `cluster` sub-object of `GET /health`. Cluster mode only.

    Attributes:
        role (str): Raft role of this node (`Leader`, `Follower`, `Candidate`, `Learner`).
        status (str): `ok` when this node sees an elected leader, `no-leader` otherwise.
        term (int):
        dim (Union[None, Unset, int]): Vector dimension the cluster locked on first insert, if any.
        leader (Union[None, Unset, int]): Node id of the leader this node currently sees, if any.
    """

    role: str
    status: str
    term: int
    dim: Union[None, Unset, int] = UNSET
    leader: Union[None, Unset, int] = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        role = self.role

        status = self.status

        term = self.term

        dim: Union[None, Unset, int]
        if isinstance(self.dim, Unset):
            dim = UNSET
        else:
            dim = self.dim

        leader: Union[None, Unset, int]
        if isinstance(self.leader, Unset):
            leader = UNSET
        else:
            leader = self.leader

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "role": role,
                "status": status,
                "term": term,
            }
        )
        if dim is not UNSET:
            field_dict["dim"] = dim
        if leader is not UNSET:
            field_dict["leader"] = leader

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)
        role = d.pop("role")

        status = d.pop("status")

        term = d.pop("term")

        def _parse_dim(data: object) -> Union[None, Unset, int]:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(Union[None, Unset, int], data)

        dim = _parse_dim(d.pop("dim", UNSET))

        def _parse_leader(data: object) -> Union[None, Unset, int]:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(Union[None, Unset, int], data)

        leader = _parse_leader(d.pop("leader", UNSET))

        cluster_health_stats = cls(
            role=role,
            status=status,
            term=term,
            dim=dim,
            leader=leader,
        )

        cluster_health_stats.additional_properties = d
        return cluster_health_stats

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
