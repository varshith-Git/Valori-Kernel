from collections.abc import Mapping
from typing import (
    TYPE_CHECKING,
    Any,
    TypeVar,
)

from attrs import define as _attrs_define
from attrs import field as _attrs_field

if TYPE_CHECKING:
    from ..models.operation_details import OperationDetails


T = TypeVar("T", bound="OperationSummary")


@_attrs_define
class OperationSummary:
    """
    Attributes:
        collection (str):
        details (OperationDetails): The `details` block of [`OperationSummary`].

            `shard_id` is populated on the cluster path only — standalone has no shard
            dimension, so it is absent there rather than defaulted to a fictitious `0`.
        id (str): Canonical v1 operation identity. Always a string (§13).
        status (str):
        timestamp_unix (int):
        timing (str):
        type_ (str):
    """

    collection: str
    details: "OperationDetails"
    id: str
    status: str
    timestamp_unix: int
    timing: str
    type_: str
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        collection = self.collection

        details = self.details.to_dict()

        id = self.id

        status = self.status

        timestamp_unix = self.timestamp_unix

        timing = self.timing

        type_ = self.type_

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "collection": collection,
                "details": details,
                "id": id,
                "status": status,
                "timestamp_unix": timestamp_unix,
                "timing": timing,
                "type": type_,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        from ..models.operation_details import OperationDetails

        d = dict(src_dict)
        collection = d.pop("collection")

        details = OperationDetails.from_dict(d.pop("details"))

        id = d.pop("id")

        status = d.pop("status")

        timestamp_unix = d.pop("timestamp_unix")

        timing = d.pop("timing")

        type_ = d.pop("type")

        operation_summary = cls(
            collection=collection,
            details=details,
            id=id,
            status=status,
            timestamp_unix=timestamp_unix,
            timing=timing,
            type_=type_,
        )

        operation_summary.additional_properties = d
        return operation_summary

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
