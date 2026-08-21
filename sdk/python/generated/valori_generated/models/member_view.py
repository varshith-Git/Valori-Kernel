from collections.abc import Mapping
from typing import Any, TypeVar

from attrs import define as _attrs_define
from attrs import field as _attrs_field

T = TypeVar("T", bound="MemberView")


@_attrs_define
class MemberView:
    """
    Attributes:
        api_addr (str):
        id (int):
        raft_addr (str):
        voter (bool):
    """

    api_addr: str
    id: int
    raft_addr: str
    voter: bool
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        api_addr = self.api_addr

        id = self.id

        raft_addr = self.raft_addr

        voter = self.voter

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "api_addr": api_addr,
                "id": id,
                "raft_addr": raft_addr,
                "voter": voter,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)
        api_addr = d.pop("api_addr")

        id = d.pop("id")

        raft_addr = d.pop("raft_addr")

        voter = d.pop("voter")

        member_view = cls(
            api_addr=api_addr,
            id=id,
            raft_addr=raft_addr,
            voter=voter,
        )

        member_view.additional_properties = d
        return member_view

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
