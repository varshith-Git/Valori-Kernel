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

T = TypeVar("T", bound="StructuredClaim")


@_attrs_define
class StructuredClaim:
    """
    Attributes:
        object_ (str):
        predicate (str):
        subject (str):
        negated (Union[Unset, bool]):
        time_scope (Union[None, Unset, str]):
    """

    object_: str
    predicate: str
    subject: str
    negated: Union[Unset, bool] = UNSET
    time_scope: Union[None, Unset, str] = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        object_ = self.object_

        predicate = self.predicate

        subject = self.subject

        negated = self.negated

        time_scope: Union[None, Unset, str]
        if isinstance(self.time_scope, Unset):
            time_scope = UNSET
        else:
            time_scope = self.time_scope

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "object": object_,
                "predicate": predicate,
                "subject": subject,
            }
        )
        if negated is not UNSET:
            field_dict["negated"] = negated
        if time_scope is not UNSET:
            field_dict["time_scope"] = time_scope

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)
        object_ = d.pop("object")

        predicate = d.pop("predicate")

        subject = d.pop("subject")

        negated = d.pop("negated", UNSET)

        def _parse_time_scope(data: object) -> Union[None, Unset, str]:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(Union[None, Unset, str], data)

        time_scope = _parse_time_scope(d.pop("time_scope", UNSET))

        structured_claim = cls(
            object_=object_,
            predicate=predicate,
            subject=subject,
            negated=negated,
            time_scope=time_scope,
        )

        structured_claim.additional_properties = d
        return structured_claim

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
