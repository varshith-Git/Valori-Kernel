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
    from ..models.stage_view import StageView


T = TypeVar("T", bound="ExecutionRecord")


@_attrs_define
class ExecutionRecord:
    """A completed ingest execution, keyed by `operation_id` — the real payload
    for `GET /v1/operations/:id/execution`.

        Attributes:
            chunks_produced (int):
            collection (str):
            document_source (str):
            operation_id (str):
            records_written (int):
            stages (list['StageView']):
            success (bool):
            total_duration_ms (int):
            error (Union[None, Unset, str]):
            receipt_id (Union[None, Unset, str]): Present when the operation's receipt was emitted before this record
                was built — always true for the standalone `/v1/ingest` path.
            state_hash_after (Union[None, Unset, str]):
            state_hash_before (Union[None, Unset, str]):
    """

    chunks_produced: int
    collection: str
    document_source: str
    operation_id: str
    records_written: int
    stages: list["StageView"]
    success: bool
    total_duration_ms: int
    error: Union[None, Unset, str] = UNSET
    receipt_id: Union[None, Unset, str] = UNSET
    state_hash_after: Union[None, Unset, str] = UNSET
    state_hash_before: Union[None, Unset, str] = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        chunks_produced = self.chunks_produced

        collection = self.collection

        document_source = self.document_source

        operation_id = self.operation_id

        records_written = self.records_written

        stages = []
        for stages_item_data in self.stages:
            stages_item = stages_item_data.to_dict()
            stages.append(stages_item)

        success = self.success

        total_duration_ms = self.total_duration_ms

        error: Union[None, Unset, str]
        if isinstance(self.error, Unset):
            error = UNSET
        else:
            error = self.error

        receipt_id: Union[None, Unset, str]
        if isinstance(self.receipt_id, Unset):
            receipt_id = UNSET
        else:
            receipt_id = self.receipt_id

        state_hash_after: Union[None, Unset, str]
        if isinstance(self.state_hash_after, Unset):
            state_hash_after = UNSET
        else:
            state_hash_after = self.state_hash_after

        state_hash_before: Union[None, Unset, str]
        if isinstance(self.state_hash_before, Unset):
            state_hash_before = UNSET
        else:
            state_hash_before = self.state_hash_before

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "chunks_produced": chunks_produced,
                "collection": collection,
                "document_source": document_source,
                "operation_id": operation_id,
                "records_written": records_written,
                "stages": stages,
                "success": success,
                "total_duration_ms": total_duration_ms,
            }
        )
        if error is not UNSET:
            field_dict["error"] = error
        if receipt_id is not UNSET:
            field_dict["receipt_id"] = receipt_id
        if state_hash_after is not UNSET:
            field_dict["state_hash_after"] = state_hash_after
        if state_hash_before is not UNSET:
            field_dict["state_hash_before"] = state_hash_before

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        from ..models.stage_view import StageView

        d = dict(src_dict)
        chunks_produced = d.pop("chunks_produced")

        collection = d.pop("collection")

        document_source = d.pop("document_source")

        operation_id = d.pop("operation_id")

        records_written = d.pop("records_written")

        stages = []
        _stages = d.pop("stages")
        for stages_item_data in _stages:
            stages_item = StageView.from_dict(stages_item_data)

            stages.append(stages_item)

        success = d.pop("success")

        total_duration_ms = d.pop("total_duration_ms")

        def _parse_error(data: object) -> Union[None, Unset, str]:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(Union[None, Unset, str], data)

        error = _parse_error(d.pop("error", UNSET))

        def _parse_receipt_id(data: object) -> Union[None, Unset, str]:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(Union[None, Unset, str], data)

        receipt_id = _parse_receipt_id(d.pop("receipt_id", UNSET))

        def _parse_state_hash_after(data: object) -> Union[None, Unset, str]:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(Union[None, Unset, str], data)

        state_hash_after = _parse_state_hash_after(d.pop("state_hash_after", UNSET))

        def _parse_state_hash_before(data: object) -> Union[None, Unset, str]:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(Union[None, Unset, str], data)

        state_hash_before = _parse_state_hash_before(d.pop("state_hash_before", UNSET))

        execution_record = cls(
            chunks_produced=chunks_produced,
            collection=collection,
            document_source=document_source,
            operation_id=operation_id,
            records_written=records_written,
            stages=stages,
            success=success,
            total_duration_ms=total_duration_ms,
            error=error,
            receipt_id=receipt_id,
            state_hash_after=state_hash_after,
            state_hash_before=state_hash_before,
        )

        execution_record.additional_properties = d
        return execution_record

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
