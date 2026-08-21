from collections.abc import Mapping
from typing import (
    Any,
    TypeVar,
    Union,
    cast,
)

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..models.ingest_job_state import IngestJobState
from ..types import UNSET, Unset

T = TypeVar("T", bound="IngestJobStatusResponse")


@_attrs_define
class IngestJobStatusResponse:
    """The body of `GET /v1/ingest/status/{job_id}`.

    # Why this type exists

    Phase API-3.3: this response was annotated `body = Object`, rendering as a
    bare `type: object` with no properties — `object` in TypeScript,
    `Dict[str, Any]` in Python. An SDK user polling an async ingest had no
    typed way to learn whether the job finished, and no discoverable name for
    the field carrying the answer. That defeats the purpose of the `202`
    contract that points here.

    Every field is optional except `status` and `job_id`, because which ones
    are present genuinely depends on the stage the job has reached — the
    terminal-success fields do not exist while it is `processing`, and `error`
    exists only on `failed`. `status` is the discriminant to branch on.

        Attributes:
            job_id (str): Echo of the polled job id.
            status (IngestJobState): The lifecycle states an asynchronous ingest job actually reports.

                Phase API-3.3: these are the three literals both routers write — see the
                `jobs.insert(..)` calls in [`ingest`] (standalone) and `cluster_ingest`
                (cluster). There is no separate `pending`: a job is `processing` from the
                moment `POST /v1/ingest?async=true` answers `202`.
            chunk_count (Union[None, Unset, int]): Chunks the document was split into.
            collection (Union[None, Unset, str]): Target collection. Absent on `failed` jobs that failed before resolving
                one.
            document_node_id (Union[None, Unset, int]): `completed` only — the graph node representing the ingested
                document.
            error (Union[None, Unset, str]): `failed` only — the human-readable reason.
            operation_id (Union[None, Unset, str]): `completed` only — correlates with `GET /v1/operations/{id}`.
            record_ids (Union[None, Unset, list[int]]): `completed` only — the records written, one per chunk.
            strategy_used (Union[None, Unset, str]): Chunking strategy the server selected.
    """

    job_id: str
    status: IngestJobState
    chunk_count: Union[None, Unset, int] = UNSET
    collection: Union[None, Unset, str] = UNSET
    document_node_id: Union[None, Unset, int] = UNSET
    error: Union[None, Unset, str] = UNSET
    operation_id: Union[None, Unset, str] = UNSET
    record_ids: Union[None, Unset, list[int]] = UNSET
    strategy_used: Union[None, Unset, str] = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        job_id = self.job_id

        status = self.status.value

        chunk_count: Union[None, Unset, int]
        if isinstance(self.chunk_count, Unset):
            chunk_count = UNSET
        else:
            chunk_count = self.chunk_count

        collection: Union[None, Unset, str]
        if isinstance(self.collection, Unset):
            collection = UNSET
        else:
            collection = self.collection

        document_node_id: Union[None, Unset, int]
        if isinstance(self.document_node_id, Unset):
            document_node_id = UNSET
        else:
            document_node_id = self.document_node_id

        error: Union[None, Unset, str]
        if isinstance(self.error, Unset):
            error = UNSET
        else:
            error = self.error

        operation_id: Union[None, Unset, str]
        if isinstance(self.operation_id, Unset):
            operation_id = UNSET
        else:
            operation_id = self.operation_id

        record_ids: Union[None, Unset, list[int]]
        if isinstance(self.record_ids, Unset):
            record_ids = UNSET
        elif isinstance(self.record_ids, list):
            record_ids = self.record_ids

        else:
            record_ids = self.record_ids

        strategy_used: Union[None, Unset, str]
        if isinstance(self.strategy_used, Unset):
            strategy_used = UNSET
        else:
            strategy_used = self.strategy_used

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "job_id": job_id,
                "status": status,
            }
        )
        if chunk_count is not UNSET:
            field_dict["chunk_count"] = chunk_count
        if collection is not UNSET:
            field_dict["collection"] = collection
        if document_node_id is not UNSET:
            field_dict["document_node_id"] = document_node_id
        if error is not UNSET:
            field_dict["error"] = error
        if operation_id is not UNSET:
            field_dict["operation_id"] = operation_id
        if record_ids is not UNSET:
            field_dict["record_ids"] = record_ids
        if strategy_used is not UNSET:
            field_dict["strategy_used"] = strategy_used

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)
        job_id = d.pop("job_id")

        status = IngestJobState(d.pop("status"))

        def _parse_chunk_count(data: object) -> Union[None, Unset, int]:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(Union[None, Unset, int], data)

        chunk_count = _parse_chunk_count(d.pop("chunk_count", UNSET))

        def _parse_collection(data: object) -> Union[None, Unset, str]:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(Union[None, Unset, str], data)

        collection = _parse_collection(d.pop("collection", UNSET))

        def _parse_document_node_id(data: object) -> Union[None, Unset, int]:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(Union[None, Unset, int], data)

        document_node_id = _parse_document_node_id(d.pop("document_node_id", UNSET))

        def _parse_error(data: object) -> Union[None, Unset, str]:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(Union[None, Unset, str], data)

        error = _parse_error(d.pop("error", UNSET))

        def _parse_operation_id(data: object) -> Union[None, Unset, str]:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(Union[None, Unset, str], data)

        operation_id = _parse_operation_id(d.pop("operation_id", UNSET))

        def _parse_record_ids(data: object) -> Union[None, Unset, list[int]]:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            try:
                if not isinstance(data, list):
                    raise TypeError()
                record_ids_type_0 = cast(list[int], data)

                return record_ids_type_0
            except:  # noqa: E722
                pass
            return cast(Union[None, Unset, list[int]], data)

        record_ids = _parse_record_ids(d.pop("record_ids", UNSET))

        def _parse_strategy_used(data: object) -> Union[None, Unset, str]:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(Union[None, Unset, str], data)

        strategy_used = _parse_strategy_used(d.pop("strategy_used", UNSET))

        ingest_job_status_response = cls(
            job_id=job_id,
            status=status,
            chunk_count=chunk_count,
            collection=collection,
            document_node_id=document_node_id,
            error=error,
            operation_id=operation_id,
            record_ids=record_ids,
            strategy_used=strategy_used,
        )

        ingest_job_status_response.additional_properties = d
        return ingest_job_status_response

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
