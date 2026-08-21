from collections.abc import Mapping
from typing import (
    TYPE_CHECKING,
    Any,
    TypeVar,
)

from attrs import define as _attrs_define
from attrs import field as _attrs_field

if TYPE_CHECKING:
    from ..models.operation_detail_response_proof import OperationDetailResponseProof
    from ..models.operation_metrics import OperationMetrics
    from ..models.operation_overview import OperationOverview
    from ..models.operation_results import OperationResults


T = TypeVar("T", bound="OperationDetailResponse")


@_attrs_define
class OperationDetailResponse:
    """
    Attributes:
        collection (str):
        id (str): Canonical v1 operation identity. Always a string (§13).
        metrics (OperationMetrics): The `metrics` block of [`OperationDetailResponse`].
        overview (OperationOverview): The `overview` block of [`OperationDetailResponse`].
        proof (OperationDetailResponseProof): The proof of the state transition.

            When a receipt was assembled for this operation this is a full
            [`crate::openapi::ReceiptDto`]. When one was not — a receipt store is
            in-process and does not survive a restart — the node synthesises a
            reduced stand-in carrying `receipt_id`, `status`, `operation_hash`,
            `state_hash_before` and `state_hash_after`. Because the two shapes
            genuinely differ, this is documented as an open object rather than
            claiming a single schema that only sometimes holds.
        results (OperationResults): The `results` block of [`OperationDetailResponse`].
        status (str):
        timestamp_unix (int):
        timing (str):
        type_ (str):
    """

    collection: str
    id: str
    metrics: "OperationMetrics"
    overview: "OperationOverview"
    proof: "OperationDetailResponseProof"
    results: "OperationResults"
    status: str
    timestamp_unix: int
    timing: str
    type_: str
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        collection = self.collection

        id = self.id

        metrics = self.metrics.to_dict()

        overview = self.overview.to_dict()

        proof = self.proof.to_dict()

        results = self.results.to_dict()

        status = self.status

        timestamp_unix = self.timestamp_unix

        timing = self.timing

        type_ = self.type_

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "collection": collection,
                "id": id,
                "metrics": metrics,
                "overview": overview,
                "proof": proof,
                "results": results,
                "status": status,
                "timestamp_unix": timestamp_unix,
                "timing": timing,
                "type": type_,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        from ..models.operation_detail_response_proof import (
            OperationDetailResponseProof,
        )
        from ..models.operation_metrics import OperationMetrics
        from ..models.operation_overview import OperationOverview
        from ..models.operation_results import OperationResults

        d = dict(src_dict)
        collection = d.pop("collection")

        id = d.pop("id")

        metrics = OperationMetrics.from_dict(d.pop("metrics"))

        overview = OperationOverview.from_dict(d.pop("overview"))

        proof = OperationDetailResponseProof.from_dict(d.pop("proof"))

        results = OperationResults.from_dict(d.pop("results"))

        status = d.pop("status")

        timestamp_unix = d.pop("timestamp_unix")

        timing = d.pop("timing")

        type_ = d.pop("type")

        operation_detail_response = cls(
            collection=collection,
            id=id,
            metrics=metrics,
            overview=overview,
            proof=proof,
            results=results,
            status=status,
            timestamp_unix=timestamp_unix,
            timing=timing,
            type_=type_,
        )

        operation_detail_response.additional_properties = d
        return operation_detail_response

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
