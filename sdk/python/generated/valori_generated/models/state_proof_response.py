from collections.abc import Mapping
from typing import Any, TypeVar

from attrs import define as _attrs_define
from attrs import field as _attrs_field

T = TypeVar("T", bound="StateProofResponse")


@_attrs_define
class StateProofResponse:
    """`GET /v1/proof/state` — the running BLAKE3 Merkle root over all applied
    events. Identical wire shape in standalone and cluster mode.

        Attributes:
            final_state_hash (str): 64 lowercase hex characters (32 bytes).
    """

    final_state_hash: str
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        final_state_hash = self.final_state_hash

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "final_state_hash": final_state_hash,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)
        final_state_hash = d.pop("final_state_hash")

        state_proof_response = cls(
            final_state_hash=final_state_hash,
        )

        state_proof_response.additional_properties = d
        return state_proof_response

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
