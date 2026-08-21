from collections.abc import Mapping
from typing import Any, TypeVar

from attrs import define as _attrs_define
from attrs import field as _attrs_field

T = TypeVar("T", bound="IngestAcceptedResponse")


@_attrs_define
class IngestAcceptedResponse:
    """The `202` body of `POST /v1/ingest` when `async: true`.

    The async branch has always returned this object; the contract used to
    declare the `202` with no content at all, so a generated client saw
    `never` and had no typed way to reach `job_id` — the one field the whole
    async flow depends on, since it is what `GET /v1/ingest/status/{job_id}`
    takes.

        Attributes:
            collection (str):
            job_id (str): Poll `GET /v1/ingest/status/{job_id}` with this id.
            ok (bool):
            status (str): Always `processing` on this response.
    """

    collection: str
    job_id: str
    ok: bool
    status: str
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        collection = self.collection

        job_id = self.job_id

        ok = self.ok

        status = self.status

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "collection": collection,
                "job_id": job_id,
                "ok": ok,
                "status": status,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)
        collection = d.pop("collection")

        job_id = d.pop("job_id")

        ok = d.pop("ok")

        status = d.pop("status")

        ingest_accepted_response = cls(
            collection=collection,
            job_id=job_id,
            ok=ok,
            status=status,
        )

        ingest_accepted_response.additional_properties = d
        return ingest_accepted_response

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
