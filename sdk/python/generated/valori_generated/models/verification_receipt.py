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

from ..models.verification_outcome import VerificationOutcome
from ..types import UNSET, Unset

if TYPE_CHECKING:
    from ..models.assertion_evidence import AssertionEvidence


T = TypeVar("T", bound="VerificationReceipt")


@_attrs_define
class VerificationReceipt:
    """
    Attributes:
        confidence_source (str):
        config_hash (str):
        evidence_refs (list['AssertionEvidence']):
        input_assertion_ids (list[str]):
        outcome (VerificationOutcome):
        receipt_hash (str):
        verification_id (str):
        verifier_type (str):
        verifier_version (str):
        confidence (Union[None, Unset, str]):
    """

    confidence_source: str
    config_hash: str
    evidence_refs: list["AssertionEvidence"]
    input_assertion_ids: list[str]
    outcome: VerificationOutcome
    receipt_hash: str
    verification_id: str
    verifier_type: str
    verifier_version: str
    confidence: Union[None, Unset, str] = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        confidence_source = self.confidence_source

        config_hash = self.config_hash

        evidence_refs = []
        for evidence_refs_item_data in self.evidence_refs:
            evidence_refs_item = evidence_refs_item_data.to_dict()
            evidence_refs.append(evidence_refs_item)

        input_assertion_ids = self.input_assertion_ids

        outcome = self.outcome.value

        receipt_hash = self.receipt_hash

        verification_id = self.verification_id

        verifier_type = self.verifier_type

        verifier_version = self.verifier_version

        confidence: Union[None, Unset, str]
        if isinstance(self.confidence, Unset):
            confidence = UNSET
        else:
            confidence = self.confidence

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "confidence_source": confidence_source,
                "config_hash": config_hash,
                "evidence_refs": evidence_refs,
                "input_assertion_ids": input_assertion_ids,
                "outcome": outcome,
                "receipt_hash": receipt_hash,
                "verification_id": verification_id,
                "verifier_type": verifier_type,
                "verifier_version": verifier_version,
            }
        )
        if confidence is not UNSET:
            field_dict["confidence"] = confidence

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        from ..models.assertion_evidence import AssertionEvidence

        d = dict(src_dict)
        confidence_source = d.pop("confidence_source")

        config_hash = d.pop("config_hash")

        evidence_refs = []
        _evidence_refs = d.pop("evidence_refs")
        for evidence_refs_item_data in _evidence_refs:
            evidence_refs_item = AssertionEvidence.from_dict(evidence_refs_item_data)

            evidence_refs.append(evidence_refs_item)

        input_assertion_ids = cast(list[str], d.pop("input_assertion_ids"))

        outcome = VerificationOutcome(d.pop("outcome"))

        receipt_hash = d.pop("receipt_hash")

        verification_id = d.pop("verification_id")

        verifier_type = d.pop("verifier_type")

        verifier_version = d.pop("verifier_version")

        def _parse_confidence(data: object) -> Union[None, Unset, str]:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(Union[None, Unset, str], data)

        confidence = _parse_confidence(d.pop("confidence", UNSET))

        verification_receipt = cls(
            confidence_source=confidence_source,
            config_hash=config_hash,
            evidence_refs=evidence_refs,
            input_assertion_ids=input_assertion_ids,
            outcome=outcome,
            receipt_hash=receipt_hash,
            verification_id=verification_id,
            verifier_type=verifier_type,
            verifier_version=verifier_version,
            confidence=confidence,
        )

        verification_receipt.additional_properties = d
        return verification_receipt

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
