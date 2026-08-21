from collections.abc import Mapping
from typing import Any, TypeVar

from attrs import define as _attrs_define
from attrs import field as _attrs_field

T = TypeVar("T", bound="OperationDetailResponseProof")


@_attrs_define
class OperationDetailResponseProof:
    """The proof of the state transition.

    When a receipt was assembled for this operation this is a full
    [`crate::openapi::ReceiptDto`]. When one was not — a receipt store is
    in-process and does not survive a restart — the node synthesises a
    reduced stand-in carrying `receipt_id`, `status`, `operation_hash`,
    `state_hash_before` and `state_hash_after`. Because the two shapes
    genuinely differ, this is documented as an open object rather than
    claiming a single schema that only sometimes holds.

    """

    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)
        operation_detail_response_proof = cls()

        operation_detail_response_proof.additional_properties = d
        return operation_detail_response_proof

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
