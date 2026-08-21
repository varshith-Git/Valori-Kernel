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

T = TypeVar("T", bound="ClusterProofResponse")


@_attrs_define
class ClusterProofResponse:
    """`GET /v1/cluster/proof` response.

    Attributes:
        final_state_hash (str): 64 lowercase hex characters (32 bytes).
        node_id (int):
        term (int):
        last_applied_index (Union[None, Unset, int]): Raft index this hash was taken at. Two peers only need to agree
            when
            compared at the same index.
    """

    final_state_hash: str
    node_id: int
    term: int
    last_applied_index: Union[None, Unset, int] = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        final_state_hash = self.final_state_hash

        node_id = self.node_id

        term = self.term

        last_applied_index: Union[None, Unset, int]
        if isinstance(self.last_applied_index, Unset):
            last_applied_index = UNSET
        else:
            last_applied_index = self.last_applied_index

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "final_state_hash": final_state_hash,
                "node_id": node_id,
                "term": term,
            }
        )
        if last_applied_index is not UNSET:
            field_dict["last_applied_index"] = last_applied_index

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)
        final_state_hash = d.pop("final_state_hash")

        node_id = d.pop("node_id")

        term = d.pop("term")

        def _parse_last_applied_index(data: object) -> Union[None, Unset, int]:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(Union[None, Unset, int], data)

        last_applied_index = _parse_last_applied_index(
            d.pop("last_applied_index", UNSET)
        )

        cluster_proof_response = cls(
            final_state_hash=final_state_hash,
            node_id=node_id,
            term=term,
            last_applied_index=last_applied_index,
        )

        cluster_proof_response.additional_properties = d
        return cluster_proof_response

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
