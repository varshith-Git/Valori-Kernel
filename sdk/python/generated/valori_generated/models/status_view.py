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
    from ..models.member_view import MemberView


T = TypeVar("T", bound="StatusView")


@_attrs_define
class StatusView:
    """
    Attributes:
        is_leader (bool):
        members (list['MemberView']):
        node_id (int):
        term (int):
        current_leader (Union[None, Unset, int]):
        last_applied_index (Union[None, Unset, int]):
        last_log_index (Union[None, Unset, int]):
    """

    is_leader: bool
    members: list["MemberView"]
    node_id: int
    term: int
    current_leader: Union[None, Unset, int] = UNSET
    last_applied_index: Union[None, Unset, int] = UNSET
    last_log_index: Union[None, Unset, int] = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        is_leader = self.is_leader

        members = []
        for members_item_data in self.members:
            members_item = members_item_data.to_dict()
            members.append(members_item)

        node_id = self.node_id

        term = self.term

        current_leader: Union[None, Unset, int]
        if isinstance(self.current_leader, Unset):
            current_leader = UNSET
        else:
            current_leader = self.current_leader

        last_applied_index: Union[None, Unset, int]
        if isinstance(self.last_applied_index, Unset):
            last_applied_index = UNSET
        else:
            last_applied_index = self.last_applied_index

        last_log_index: Union[None, Unset, int]
        if isinstance(self.last_log_index, Unset):
            last_log_index = UNSET
        else:
            last_log_index = self.last_log_index

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "is_leader": is_leader,
                "members": members,
                "node_id": node_id,
                "term": term,
            }
        )
        if current_leader is not UNSET:
            field_dict["current_leader"] = current_leader
        if last_applied_index is not UNSET:
            field_dict["last_applied_index"] = last_applied_index
        if last_log_index is not UNSET:
            field_dict["last_log_index"] = last_log_index

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        from ..models.member_view import MemberView

        d = dict(src_dict)
        is_leader = d.pop("is_leader")

        members = []
        _members = d.pop("members")
        for members_item_data in _members:
            members_item = MemberView.from_dict(members_item_data)

            members.append(members_item)

        node_id = d.pop("node_id")

        term = d.pop("term")

        def _parse_current_leader(data: object) -> Union[None, Unset, int]:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(Union[None, Unset, int], data)

        current_leader = _parse_current_leader(d.pop("current_leader", UNSET))

        def _parse_last_applied_index(data: object) -> Union[None, Unset, int]:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(Union[None, Unset, int], data)

        last_applied_index = _parse_last_applied_index(
            d.pop("last_applied_index", UNSET)
        )

        def _parse_last_log_index(data: object) -> Union[None, Unset, int]:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(Union[None, Unset, int], data)

        last_log_index = _parse_last_log_index(d.pop("last_log_index", UNSET))

        status_view = cls(
            is_leader=is_leader,
            members=members,
            node_id=node_id,
            term=term,
            current_leader=current_leader,
            last_applied_index=last_applied_index,
            last_log_index=last_log_index,
        )

        status_view.additional_properties = d
        return status_view

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
