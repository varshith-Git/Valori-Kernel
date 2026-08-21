from collections.abc import Mapping
from typing import (
    TYPE_CHECKING,
    Any,
    TypeVar,
)

from attrs import define as _attrs_define
from attrs import field as _attrs_field

if TYPE_CHECKING:
    from ..models.usage_storage import UsageStorage


T = TypeVar("T", bound="UsageResponse")


@_attrs_define
class UsageResponse:
    """`GET /v1/usage` — raw counters only. The node is plan-agnostic: it never
    returns quota, plan, or billing context.

        Attributes:
            collections (int):
            records (int):
            storage (UsageStorage): The `storage` sub-object of `GET /v1/usage`.
    """

    collections: int
    records: int
    storage: "UsageStorage"
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        collections = self.collections

        records = self.records

        storage = self.storage.to_dict()

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "collections": collections,
                "records": records,
                "storage": storage,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        from ..models.usage_storage import UsageStorage

        d = dict(src_dict)
        collections = d.pop("collections")

        records = d.pop("records")

        storage = UsageStorage.from_dict(d.pop("storage"))

        usage_response = cls(
            collections=collections,
            records=records,
            storage=storage,
        )

        usage_response.additional_properties = d
        return usage_response

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
