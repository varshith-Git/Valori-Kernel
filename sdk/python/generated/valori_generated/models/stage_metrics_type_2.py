from collections.abc import Mapping
from typing import Any, TypeVar

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..models.stage_metrics_type_2_stage import StageMetricsType2Stage

T = TypeVar("T", bound="StageMetricsType2")


@_attrs_define
class StageMetricsType2:
    """
    Attributes:
        avg_chunk_bytes (int):
        chunks_created (int):
        max_chunk_bytes (int):
        stage (StageMetricsType2Stage):
    """

    avg_chunk_bytes: int
    chunks_created: int
    max_chunk_bytes: int
    stage: StageMetricsType2Stage
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        avg_chunk_bytes = self.avg_chunk_bytes

        chunks_created = self.chunks_created

        max_chunk_bytes = self.max_chunk_bytes

        stage = self.stage.value

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "avg_chunk_bytes": avg_chunk_bytes,
                "chunks_created": chunks_created,
                "max_chunk_bytes": max_chunk_bytes,
                "stage": stage,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)
        avg_chunk_bytes = d.pop("avg_chunk_bytes")

        chunks_created = d.pop("chunks_created")

        max_chunk_bytes = d.pop("max_chunk_bytes")

        stage = StageMetricsType2Stage(d.pop("stage"))

        stage_metrics_type_2 = cls(
            avg_chunk_bytes=avg_chunk_bytes,
            chunks_created=chunks_created,
            max_chunk_bytes=max_chunk_bytes,
            stage=stage,
        )

        stage_metrics_type_2.additional_properties = d
        return stage_metrics_type_2

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
