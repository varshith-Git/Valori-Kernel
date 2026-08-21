from collections.abc import Mapping
from typing import Any, TypeVar

from attrs import define as _attrs_define
from attrs import field as _attrs_field

T = TypeVar("T", bound="IndexRebuildResponse")


@_attrs_define
class IndexRebuildResponse:
    """`POST /v1/index/rebuild` response.

    Attributes:
        effective (str): The `index` value from the request, or `"rebuilt"` when it was absent.
        ok (bool):
        records (int): Live record count after the rebuild.
    """

    effective: str
    ok: bool
    records: int
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        effective = self.effective

        ok = self.ok

        records = self.records

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "effective": effective,
                "ok": ok,
                "records": records,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)
        effective = d.pop("effective")

        ok = d.pop("ok")

        records = d.pop("records")

        index_rebuild_response = cls(
            effective=effective,
            ok=ok,
            records=records,
        )

        index_rebuild_response.additional_properties = d
        return index_rebuild_response

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
