# Copyright (c) 2025 Varshith Gudur. Licensed under MIT OR Apache-2.0.
"""Record operations, scoped to a collection.

Covers ``insert_record``, ``insert_records_batch``, ``insert_encrypted_record``,
``get_record``, ``delete_record``, ``soft_delete_record`` and
``update_record_metadata``.
"""

from __future__ import annotations

from typing import Any, Mapping, Optional, Sequence

from valori_generated.api.records import (
    delete_record as _delete_record,
    get_record as _get_record,
    insert_encrypted_record as _insert_encrypted_record,
    insert_record as _insert_record,
    insert_records_batch as _insert_records_batch,
    soft_delete_record as _soft_delete_record,
    update_record_metadata as _update_record_metadata,
)
from valori_generated.models.batch_insert_request import BatchInsertRequest
from valori_generated.models.delete_record_request import DeleteRecordRequest
from valori_generated.models.insert_encrypted_request import InsertEncryptedRequest
from valori_generated.models.insert_record_request import InsertRecordRequest
from valori_generated.models.update_record_metadata_body import UpdateRecordMetadataBody

from .._models import build
from .._wire import (
    encode_metadata_bytes,
    encode_metadata_object,
    encode_metadata_string_list,
)
from ..transport import unset_if_none
from ._base import CollectionScoped

__all__ = ["Records"]


class Records(CollectionScoped):
    """``collection.records`` — insert, read, delete and amend records."""

    def insert(
        self,
        values: Sequence[float],
        *,
        metadata: Optional[Mapping[str, Any]] = None,
        text: Optional[str] = None,
        tag: Optional[int] = None,
        request_id: Optional[str] = None,
    ) -> Any:
        """Insert one record. ``POST /v1/records``.

        ``request_id`` is the server's dedup key. Supplying one is also what
        makes this write eligible for automatic retry (§8) — without it the SDK
        will not repeat the request, because a repeat could double-insert.

        ``metadata`` is an ordinary mapping here; the wire takes opaque UTF-8
        JSON bytes. See :mod:`valori._wire`.
        """
        body = build(
            InsertRecordRequest,
            values=list(values),
            collection=self._collection,
            metadata=encode_metadata_bytes(metadata),
            text=text,
            tag=tag,
            request_id=request_id,
        )
        return self._t.call(_insert_record, body=body, request_id=request_id)

    def insert_batch(
        self,
        batch: Sequence[Sequence[float]],
        *,
        metadata: Optional[Sequence[Optional[Mapping[str, Any]]]] = None,
        texts: Optional[Sequence[str]] = None,
        request_ids: Optional[Sequence[str]] = None,
    ) -> Any:
        """Insert many records in one call. ``POST /v1/vectors/batch-insert``.

        Each ``metadata`` entry is an ordinary mapping (or ``None`` for "no
        metadata for this vector"); the wire takes UTF-8 JSON strings. See
        :mod:`valori._wire`.
        """
        body = build(
            BatchInsertRequest,
            batch=[list(v) for v in batch],
            collection=self._collection,
            metadata=encode_metadata_string_list(metadata),
            texts=list(texts) if texts is not None else None,
            request_ids=list(request_ids) if request_ids is not None else None,
        )
        return self._t.call(_insert_records_batch, body=body)

    def insert_encrypted(
        self,
        payload: str,
        *,
        key_id: Optional[str] = None,
        tag: Optional[int] = None,
    ) -> Any:
        """Insert an already-encrypted payload. ``POST /v1/records/encrypted``."""
        body = build(
            InsertEncryptedRequest,
            payload=payload,
            collection=self._collection,
            key_id=key_id,
            tag=tag,
        )
        return self._t.call(_insert_encrypted_record, body=body)

    def get(self, record_id: int) -> Any:
        """Fetch one record by id. ``GET /v1/records/{id}``."""
        return self._t.call(
            _get_record, id=record_id, collection=unset_if_none(self._collection)
        )

    def delete(self, record_id: int) -> Any:
        """Hard-delete a record. ``POST /v1/delete``."""
        body = build(DeleteRecordRequest, id=record_id, collection=self._collection)
        return self._t.call(_delete_record, body=body)

    def soft_delete(self, record_id: int) -> Any:
        """Tombstone a record without reclaiming its slot. ``POST /v1/soft-delete``."""
        body = build(DeleteRecordRequest, id=record_id, collection=self._collection)
        return self._t.call(_soft_delete_record, body=body)

    def update_metadata(self, record_id: int, metadata: Mapping[str, Any]) -> Any:
        """Replace a record's metadata sidecar. ``PATCH /v1/records/{id}/metadata``.

        This endpoint's ``metadata`` is a real JSON object on the wire, so the
        mapping is sent verbatim — but it is still routed through the wire layer
        so it is validated at the boundary rather than coerced by the generated
        model's permissive ``from_dict``.
        """
        encoded = encode_metadata_object(metadata)
        body = UpdateRecordMetadataBody.from_dict(encoded if encoded is not None else {})
        return self._t.call(
            _update_record_metadata,
            id=record_id,
            body=body,
            collection=unset_if_none(self._collection),
        )
