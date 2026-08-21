from collections.abc import Mapping
from typing import Any, TypeVar

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..models.stage_metrics_type_0_stage import StageMetricsType0Stage

T = TypeVar("T", bound="StageMetricsType0")


@_attrs_define
class StageMetricsType0:
    """
    Attributes:
        bytes_read (int):
        mime (str):
        stage (StageMetricsType0Stage):
    """

    bytes_read: int
    mime: str
    stage: StageMetricsType0Stage
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        bytes_read = self.bytes_read

        mime = self.mime

        stage = self.stage.value

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "bytes_read": bytes_read,
                "mime": mime,
                "stage": stage,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)
        bytes_read = d.pop("bytes_read")

        mime = d.pop("mime")

        stage = StageMetricsType0Stage(d.pop("stage"))

        stage_metrics_type_0 = cls(
            bytes_read=bytes_read,
            mime=mime,
            stage=stage,
        )

        stage_metrics_type_0.additional_properties = d
        return stage_metrics_type_0

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
