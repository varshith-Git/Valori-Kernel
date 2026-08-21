from collections.abc import Mapping
from typing import (
    TYPE_CHECKING,
    Any,
    TypeVar,
)

from attrs import define as _attrs_define
from attrs import field as _attrs_field

if TYPE_CHECKING:
    from ..models.operation_summary import OperationSummary


T = TypeVar("T", bound="OperationsListResponse")


@_attrs_define
class OperationsListResponse:
    """
    Attributes:
        operations (list['OperationSummary']):
        total (int):
    """

    operations: list["OperationSummary"]
    total: int
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        operations = []
        for operations_item_data in self.operations:
            operations_item = operations_item_data.to_dict()
            operations.append(operations_item)

        total = self.total

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "operations": operations,
                "total": total,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        from ..models.operation_summary import OperationSummary

        d = dict(src_dict)
        operations = []
        _operations = d.pop("operations")
        for operations_item_data in _operations:
            operations_item = OperationSummary.from_dict(operations_item_data)

            operations.append(operations_item)

        total = d.pop("total")

        operations_list_response = cls(
            operations=operations,
            total=total,
        )

        operations_list_response.additional_properties = d
        return operations_list_response

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
