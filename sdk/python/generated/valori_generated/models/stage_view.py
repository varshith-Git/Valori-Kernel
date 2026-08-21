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

from ..models.stage_name import StageName
from ..types import UNSET, Unset

if TYPE_CHECKING:
    from ..models.stage_metrics_type_0 import StageMetricsType0
    from ..models.stage_metrics_type_1 import StageMetricsType1
    from ..models.stage_metrics_type_2 import StageMetricsType2
    from ..models.stage_metrics_type_3 import StageMetricsType3
    from ..models.stage_metrics_type_4 import StageMetricsType4


T = TypeVar("T", bound="StageView")


@_attrs_define
class StageView:
    """One stage, with its human-facing label alongside the full metrics —
    enough to render either a DAG step or a timeline row from the same data.

        Attributes:
            duration_ms (int):
            label (str): User-facing description ("Read document", "Generate embeddings", …) —
                never an internal crate/struct name.
            metrics (Union['StageMetricsType0', 'StageMetricsType1', 'StageMetricsType2', 'StageMetricsType3',
                'StageMetricsType4']): Format-specific counters emitted by each stage. E4.1.
            stage (StageName):
            started_at_ms (int):
            success (bool):
            warnings (list[str]):
            error (Union[None, Unset, str]):
    """

    duration_ms: int
    label: str
    metrics: Union[
        "StageMetricsType0",
        "StageMetricsType1",
        "StageMetricsType2",
        "StageMetricsType3",
        "StageMetricsType4",
    ]
    stage: StageName
    started_at_ms: int
    success: bool
    warnings: list[str]
    error: Union[None, Unset, str] = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        from ..models.stage_metrics_type_0 import StageMetricsType0
        from ..models.stage_metrics_type_1 import StageMetricsType1
        from ..models.stage_metrics_type_2 import StageMetricsType2
        from ..models.stage_metrics_type_3 import StageMetricsType3

        duration_ms = self.duration_ms

        label = self.label

        metrics: dict[str, Any]
        if isinstance(self.metrics, StageMetricsType0):
            metrics = self.metrics.to_dict()
        elif isinstance(self.metrics, StageMetricsType1):
            metrics = self.metrics.to_dict()
        elif isinstance(self.metrics, StageMetricsType2):
            metrics = self.metrics.to_dict()
        elif isinstance(self.metrics, StageMetricsType3):
            metrics = self.metrics.to_dict()
        else:
            metrics = self.metrics.to_dict()

        stage = self.stage.value

        started_at_ms = self.started_at_ms

        success = self.success

        warnings = self.warnings

        error: Union[None, Unset, str]
        if isinstance(self.error, Unset):
            error = UNSET
        else:
            error = self.error

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "duration_ms": duration_ms,
                "label": label,
                "metrics": metrics,
                "stage": stage,
                "started_at_ms": started_at_ms,
                "success": success,
                "warnings": warnings,
            }
        )
        if error is not UNSET:
            field_dict["error"] = error

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        from ..models.stage_metrics_type_0 import StageMetricsType0
        from ..models.stage_metrics_type_1 import StageMetricsType1
        from ..models.stage_metrics_type_2 import StageMetricsType2
        from ..models.stage_metrics_type_3 import StageMetricsType3
        from ..models.stage_metrics_type_4 import StageMetricsType4

        d = dict(src_dict)
        duration_ms = d.pop("duration_ms")

        label = d.pop("label")

        def _parse_metrics(
            data: object,
        ) -> Union[
            "StageMetricsType0",
            "StageMetricsType1",
            "StageMetricsType2",
            "StageMetricsType3",
            "StageMetricsType4",
        ]:
            try:
                if not isinstance(data, dict):
                    raise TypeError()
                componentsschemas_stage_metrics_type_0 = StageMetricsType0.from_dict(
                    data
                )

                return componentsschemas_stage_metrics_type_0
            except:  # noqa: E722
                pass
            try:
                if not isinstance(data, dict):
                    raise TypeError()
                componentsschemas_stage_metrics_type_1 = StageMetricsType1.from_dict(
                    data
                )

                return componentsschemas_stage_metrics_type_1
            except:  # noqa: E722
                pass
            try:
                if not isinstance(data, dict):
                    raise TypeError()
                componentsschemas_stage_metrics_type_2 = StageMetricsType2.from_dict(
                    data
                )

                return componentsschemas_stage_metrics_type_2
            except:  # noqa: E722
                pass
            try:
                if not isinstance(data, dict):
                    raise TypeError()
                componentsschemas_stage_metrics_type_3 = StageMetricsType3.from_dict(
                    data
                )

                return componentsschemas_stage_metrics_type_3
            except:  # noqa: E722
                pass
            if not isinstance(data, dict):
                raise TypeError()
            componentsschemas_stage_metrics_type_4 = StageMetricsType4.from_dict(data)

            return componentsschemas_stage_metrics_type_4

        metrics = _parse_metrics(d.pop("metrics"))

        stage = StageName(d.pop("stage"))

        started_at_ms = d.pop("started_at_ms")

        success = d.pop("success")

        warnings = cast(list[str], d.pop("warnings"))

        def _parse_error(data: object) -> Union[None, Unset, str]:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(Union[None, Unset, str], data)

        error = _parse_error(d.pop("error", UNSET))

        stage_view = cls(
            duration_ms=duration_ms,
            label=label,
            metrics=metrics,
            stage=stage,
            started_at_ms=started_at_ms,
            success=success,
            warnings=warnings,
            error=error,
        )

        stage_view.additional_properties = d
        return stage_view

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
