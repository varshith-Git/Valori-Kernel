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

T = TypeVar("T", bound="EventProofResponse")


@_attrs_define
class EventProofResponse:
    """
    Attributes:
        committed_height (int):
        event_count (int):
        event_log_hash (str):
        final_state_hash (str):
        kernel_version (int):
        snapshot_hash (Union[None, Unset, str]):
    """

    committed_height: int
    event_count: int
    event_log_hash: str
    final_state_hash: str
    kernel_version: int
    snapshot_hash: Union[None, Unset, str] = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        committed_height = self.committed_height

        event_count = self.event_count

        event_log_hash = self.event_log_hash

        final_state_hash = self.final_state_hash

        kernel_version = self.kernel_version

        snapshot_hash: Union[None, Unset, str]
        if isinstance(self.snapshot_hash, Unset):
            snapshot_hash = UNSET
        else:
            snapshot_hash = self.snapshot_hash

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "committed_height": committed_height,
                "event_count": event_count,
                "event_log_hash": event_log_hash,
                "final_state_hash": final_state_hash,
                "kernel_version": kernel_version,
            }
        )
        if snapshot_hash is not UNSET:
            field_dict["snapshot_hash"] = snapshot_hash

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)
        committed_height = d.pop("committed_height")

        event_count = d.pop("event_count")

        event_log_hash = d.pop("event_log_hash")

        final_state_hash = d.pop("final_state_hash")

        kernel_version = d.pop("kernel_version")

        def _parse_snapshot_hash(data: object) -> Union[None, Unset, str]:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(Union[None, Unset, str], data)

        snapshot_hash = _parse_snapshot_hash(d.pop("snapshot_hash", UNSET))

        event_proof_response = cls(
            committed_height=committed_height,
            event_count=event_count,
            event_log_hash=event_log_hash,
            final_state_hash=final_state_hash,
            kernel_version=kernel_version,
            snapshot_hash=snapshot_hash,
        )

        event_proof_response.additional_properties = d
        return event_proof_response

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
