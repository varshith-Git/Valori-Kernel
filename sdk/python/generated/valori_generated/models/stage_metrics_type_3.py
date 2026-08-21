from collections.abc import Mapping
from typing import Any, TypeVar

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..models.stage_metrics_type_3_stage import StageMetricsType3Stage

T = TypeVar("T", bound="StageMetricsType3")


@_attrs_define
class StageMetricsType3:
    """
    Attributes:
        batch_count (int): Number of embed-batch calls made (1 per batch).
        dimensions (int):
        latency_ms (int): Wall-clock latency of all embed calls combined, milliseconds.
        model (str): Model name (e.g. `"nomic-embed-text"`).
        provider (str): Provider kind (e.g. `"ollama"`, `"openai"`), parsed from the
            first embedding's `model_id` (`"{provider}/{model}"`).
        stage (StageMetricsType3Stage):
    """

    batch_count: int
    dimensions: int
    latency_ms: int
    model: str
    provider: str
    stage: StageMetricsType3Stage
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        batch_count = self.batch_count

        dimensions = self.dimensions

        latency_ms = self.latency_ms

        model = self.model

        provider = self.provider

        stage = self.stage.value

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "batch_count": batch_count,
                "dimensions": dimensions,
                "latency_ms": latency_ms,
                "model": model,
                "provider": provider,
                "stage": stage,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)
        batch_count = d.pop("batch_count")

        dimensions = d.pop("dimensions")

        latency_ms = d.pop("latency_ms")

        model = d.pop("model")

        provider = d.pop("provider")

        stage = StageMetricsType3Stage(d.pop("stage"))

        stage_metrics_type_3 = cls(
            batch_count=batch_count,
            dimensions=dimensions,
            latency_ms=latency_ms,
            model=model,
            provider=provider,
            stage=stage,
        )

        stage_metrics_type_3.additional_properties = d
        return stage_metrics_type_3

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
