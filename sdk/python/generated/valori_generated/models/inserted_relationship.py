from collections.abc import Mapping
from typing import (
    TYPE_CHECKING,
    Any,
    TypeVar,
)

from attrs import define as _attrs_define
from attrs import field as _attrs_field

if TYPE_CHECKING:
    from ..models.assertion_evidence import AssertionEvidence


T = TypeVar("T", bound="InsertedRelationship")


@_attrs_define
class InsertedRelationship:
    """
    Attributes:
        assertion_id (str):
        description (str):
        edge_id (int):
        evidence (AssertionEvidence):
        source_name (str):
        strength (float):
        target_name (str):
    """

    assertion_id: str
    description: str
    edge_id: int
    evidence: "AssertionEvidence"
    source_name: str
    strength: float
    target_name: str
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        assertion_id = self.assertion_id

        description = self.description

        edge_id = self.edge_id

        evidence = self.evidence.to_dict()

        source_name = self.source_name

        strength = self.strength

        target_name = self.target_name

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "assertion_id": assertion_id,
                "description": description,
                "edge_id": edge_id,
                "evidence": evidence,
                "source_name": source_name,
                "strength": strength,
                "target_name": target_name,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        from ..models.assertion_evidence import AssertionEvidence

        d = dict(src_dict)
        assertion_id = d.pop("assertion_id")

        description = d.pop("description")

        edge_id = d.pop("edge_id")

        evidence = AssertionEvidence.from_dict(d.pop("evidence"))

        source_name = d.pop("source_name")

        strength = d.pop("strength")

        target_name = d.pop("target_name")

        inserted_relationship = cls(
            assertion_id=assertion_id,
            description=description,
            edge_id=edge_id,
            evidence=evidence,
            source_name=source_name,
            strength=strength,
            target_name=target_name,
        )

        inserted_relationship.additional_properties = d
        return inserted_relationship

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
