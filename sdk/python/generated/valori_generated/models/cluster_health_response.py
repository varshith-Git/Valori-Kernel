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

T = TypeVar("T", bound="ClusterHealthResponse")


@_attrs_define
class ClusterHealthResponse:
    """`GET /v1/cluster/health` response.

    Attributes:
        status (str): `ok` when a leader is visible, `no-leader` otherwise.
        detail (Union[None, Unset, str]): Present only on the `no-leader` path.
        leader (Union[None, Unset, int]):
    """

    status: str
    detail: Union[None, Unset, str] = UNSET
    leader: Union[None, Unset, int] = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        status = self.status

        detail: Union[None, Unset, str]
        if isinstance(self.detail, Unset):
            detail = UNSET
        else:
            detail = self.detail

        leader: Union[None, Unset, int]
        if isinstance(self.leader, Unset):
            leader = UNSET
        else:
            leader = self.leader

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "status": status,
            }
        )
        if detail is not UNSET:
            field_dict["detail"] = detail
        if leader is not UNSET:
            field_dict["leader"] = leader

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)
        status = d.pop("status")

        def _parse_detail(data: object) -> Union[None, Unset, str]:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(Union[None, Unset, str], data)

        detail = _parse_detail(d.pop("detail", UNSET))

        def _parse_leader(data: object) -> Union[None, Unset, int]:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(Union[None, Unset, int], data)

        leader = _parse_leader(d.pop("leader", UNSET))

        cluster_health_response = cls(
            status=status,
            detail=detail,
            leader=leader,
        )

        cluster_health_response.additional_properties = d
        return cluster_health_response

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
