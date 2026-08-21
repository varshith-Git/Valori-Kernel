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

T = TypeVar("T", bound="ClusterRoleResponse")


@_attrs_define
class ClusterRoleResponse:
    """`GET /v1/cluster/role` response.

    Attributes:
        node_id (int):
        role (str): `leader` or `follower`. Both are healthy.
        current_leader (Union[None, Unset, int]):
    """

    node_id: int
    role: str
    current_leader: Union[None, Unset, int] = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        node_id = self.node_id

        role = self.role

        current_leader: Union[None, Unset, int]
        if isinstance(self.current_leader, Unset):
            current_leader = UNSET
        else:
            current_leader = self.current_leader

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "node_id": node_id,
                "role": role,
            }
        )
        if current_leader is not UNSET:
            field_dict["current_leader"] = current_leader

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)
        node_id = d.pop("node_id")

        role = d.pop("role")

        def _parse_current_leader(data: object) -> Union[None, Unset, int]:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(Union[None, Unset, int], data)

        current_leader = _parse_current_leader(d.pop("current_leader", UNSET))

        cluster_role_response = cls(
            node_id=node_id,
            role=role,
            current_leader=current_leader,
        )

        cluster_role_response.additional_properties = d
        return cluster_role_response

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
