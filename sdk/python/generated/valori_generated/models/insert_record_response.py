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

from ..types import UNSET, Unset

if TYPE_CHECKING:
    from ..models.insert_receipt_json import InsertReceiptJson


T = TypeVar("T", bound="InsertRecordResponse")


@_attrs_define
class InsertRecordResponse:
    """**The** public response body for `POST /v1/records` — one model, both routers.

    `log_index` is Raft-only and omitted in standalone; `deduplicated` is
    present on both paths and is `true` exactly when the request carried a
    `request_id` that had already been applied, in which case `id` is the
    record the original request created and no new write happened.

        Attributes:
            deduplicated (bool): `true` when this request was recognised as a replay of a previous
                `request_id` and no new record was created.
            id (int):
            receipt (InsertReceiptJson):
            log_index (Union[None, Unset, int]): Raft log index of the committed write — cluster path only.
    """

    deduplicated: bool
    id: int
    receipt: "InsertReceiptJson"
    log_index: Union[None, Unset, int] = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        deduplicated = self.deduplicated

        id = self.id

        receipt = self.receipt.to_dict()

        log_index: Union[None, Unset, int]
        if isinstance(self.log_index, Unset):
            log_index = UNSET
        else:
            log_index = self.log_index

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "deduplicated": deduplicated,
                "id": id,
                "receipt": receipt,
            }
        )
        if log_index is not UNSET:
            field_dict["log_index"] = log_index

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        from ..models.insert_receipt_json import InsertReceiptJson

        d = dict(src_dict)
        deduplicated = d.pop("deduplicated")

        id = d.pop("id")

        receipt = InsertReceiptJson.from_dict(d.pop("receipt"))

        def _parse_log_index(data: object) -> Union[None, Unset, int]:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(Union[None, Unset, int], data)

        log_index = _parse_log_index(d.pop("log_index", UNSET))

        insert_record_response = cls(
            deduplicated=deduplicated,
            id=id,
            receipt=receipt,
            log_index=log_index,
        )

        insert_record_response.additional_properties = d
        return insert_record_response

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
