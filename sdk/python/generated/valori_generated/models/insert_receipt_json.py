from collections.abc import Mapping
from typing import Any, TypeVar

from attrs import define as _attrs_define
from attrs import field as _attrs_field

T = TypeVar("T", bound="InsertReceiptJson")


@_attrs_define
class InsertReceiptJson:
    """
    Attributes:
        new_root (str):
        old_root (str):
        proof (str):
        record_id (int):
        sequence (int):
        state_hash (str):
        timestamp (int):
    """

    new_root: str
    old_root: str
    proof: str
    record_id: int
    sequence: int
    state_hash: str
    timestamp: int
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        new_root = self.new_root

        old_root = self.old_root

        proof = self.proof

        record_id = self.record_id

        sequence = self.sequence

        state_hash = self.state_hash

        timestamp = self.timestamp

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "new_root": new_root,
                "old_root": old_root,
                "proof": proof,
                "record_id": record_id,
                "sequence": sequence,
                "state_hash": state_hash,
                "timestamp": timestamp,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)
        new_root = d.pop("new_root")

        old_root = d.pop("old_root")

        proof = d.pop("proof")

        record_id = d.pop("record_id")

        sequence = d.pop("sequence")

        state_hash = d.pop("state_hash")

        timestamp = d.pop("timestamp")

        insert_receipt_json = cls(
            new_root=new_root,
            old_root=old_root,
            proof=proof,
            record_id=record_id,
            sequence=sequence,
            state_hash=state_hash,
            timestamp=timestamp,
        )

        insert_receipt_json.additional_properties = d
        return insert_receipt_json

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
