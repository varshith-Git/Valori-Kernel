from collections.abc import Mapping
from typing import (
    TYPE_CHECKING,
    Any,
    TypeVar,
    Union,
)

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

if TYPE_CHECKING:
    from ..models.assertion_evidence import AssertionEvidence
    from ..models.structured_claim import StructuredClaim


T = TypeVar("T", bound="VerifyClaimRequest")


@_attrs_define
class VerifyClaimRequest:
    """
    Attributes:
        left (StructuredClaim):
        left_assertion_id (str):
        right (StructuredClaim):
        right_assertion_id (str):
        evidence_refs (Union[Unset, list['AssertionEvidence']]):
    """

    left: "StructuredClaim"
    left_assertion_id: str
    right: "StructuredClaim"
    right_assertion_id: str
    evidence_refs: Union[Unset, list["AssertionEvidence"]] = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        left = self.left.to_dict()

        left_assertion_id = self.left_assertion_id

        right = self.right.to_dict()

        right_assertion_id = self.right_assertion_id

        evidence_refs: Union[Unset, list[dict[str, Any]]] = UNSET
        if not isinstance(self.evidence_refs, Unset):
            evidence_refs = []
            for evidence_refs_item_data in self.evidence_refs:
                evidence_refs_item = evidence_refs_item_data.to_dict()
                evidence_refs.append(evidence_refs_item)

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "left": left,
                "left_assertion_id": left_assertion_id,
                "right": right,
                "right_assertion_id": right_assertion_id,
            }
        )
        if evidence_refs is not UNSET:
            field_dict["evidence_refs"] = evidence_refs

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        from ..models.assertion_evidence import AssertionEvidence
        from ..models.structured_claim import StructuredClaim

        d = dict(src_dict)
        left = StructuredClaim.from_dict(d.pop("left"))

        left_assertion_id = d.pop("left_assertion_id")

        right = StructuredClaim.from_dict(d.pop("right"))

        right_assertion_id = d.pop("right_assertion_id")

        evidence_refs = []
        _evidence_refs = d.pop("evidence_refs", UNSET)
        for evidence_refs_item_data in _evidence_refs or []:
            evidence_refs_item = AssertionEvidence.from_dict(evidence_refs_item_data)

            evidence_refs.append(evidence_refs_item)

        verify_claim_request = cls(
            left=left,
            left_assertion_id=left_assertion_id,
            right=right,
            right_assertion_id=right_assertion_id,
            evidence_refs=evidence_refs,
        )

        verify_claim_request.additional_properties = d
        return verify_claim_request

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
