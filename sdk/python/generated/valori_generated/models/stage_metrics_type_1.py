from collections.abc import Mapping
from typing import (
    Any,
    TypeVar,
    cast,
)

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..models.stage_metrics_type_1_stage import StageMetricsType1Stage

T = TypeVar("T", bound="StageMetricsType1")


@_attrs_define
class StageMetricsType1:
    """
    Attributes:
        checks_run (int): Number of checks that were evaluated.
        stage (StageMetricsType1Stage):
        warnings (list[str]):
    """

    checks_run: int
    stage: StageMetricsType1Stage
    warnings: list[str]
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        checks_run = self.checks_run

        stage = self.stage.value

        warnings = self.warnings

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "checks_run": checks_run,
                "stage": stage,
                "warnings": warnings,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)
        checks_run = d.pop("checks_run")

        stage = StageMetricsType1Stage(d.pop("stage"))

        warnings = cast(list[str], d.pop("warnings"))

        stage_metrics_type_1 = cls(
            checks_run=checks_run,
            stage=stage,
            warnings=warnings,
        )

        stage_metrics_type_1.additional_properties = d
        return stage_metrics_type_1

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
