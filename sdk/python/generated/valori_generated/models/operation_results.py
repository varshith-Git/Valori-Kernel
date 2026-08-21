from collections.abc import Mapping
from typing import Any, TypeVar

from attrs import define as _attrs_define
from attrs import field as _attrs_field

T = TypeVar("T", bound="OperationResults")


@_attrs_define
class OperationResults:
    """The `results` block of [`OperationDetailResponse`].

    Attributes:
        edges_affected (int):
        message (str): Human-readable summary. Do not parse.
        nodes_affected (int):
        records_affected (int):
        status (str): Commit outcome, e.g. `committed`.
    """

    edges_affected: int
    message: str
    nodes_affected: int
    records_affected: int
    status: str
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        edges_affected = self.edges_affected

        message = self.message

        nodes_affected = self.nodes_affected

        records_affected = self.records_affected

        status = self.status

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "edges_affected": edges_affected,
                "message": message,
                "nodes_affected": nodes_affected,
                "records_affected": records_affected,
                "status": status,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)
        edges_affected = d.pop("edges_affected")

        message = d.pop("message")

        nodes_affected = d.pop("nodes_affected")

        records_affected = d.pop("records_affected")

        status = d.pop("status")

        operation_results = cls(
            edges_affected=edges_affected,
            message=message,
            nodes_affected=nodes_affected,
            records_affected=records_affected,
            status=status,
        )

        operation_results.additional_properties = d
        return operation_results

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
