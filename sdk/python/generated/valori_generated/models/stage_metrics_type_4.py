from collections.abc import Mapping
from typing import Any, TypeVar

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..models.stage_metrics_type_4_stage import StageMetricsType4Stage

T = TypeVar("T", bound="StageMetricsType4")


@_attrs_define
class StageMetricsType4:
    """
    Attributes:
        graph_edges_created (int): Parent→chunk edges created. Today always equal to
            `graph_nodes_created` (`KernelWriter` creates exactly one edge per
            chunk node), tracked separately since that's an implementation
            detail of one `Writer`, not a pipeline invariant.
        graph_nodes_created (int): Chunk graph nodes created (one per written chunk that got a node —
            `KernelWriter` always does; other writers may not).
        records_written (int):
        stage (StageMetricsType4Stage):
    """

    graph_edges_created: int
    graph_nodes_created: int
    records_written: int
    stage: StageMetricsType4Stage
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        graph_edges_created = self.graph_edges_created

        graph_nodes_created = self.graph_nodes_created

        records_written = self.records_written

        stage = self.stage.value

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "graph_edges_created": graph_edges_created,
                "graph_nodes_created": graph_nodes_created,
                "records_written": records_written,
                "stage": stage,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)
        graph_edges_created = d.pop("graph_edges_created")

        graph_nodes_created = d.pop("graph_nodes_created")

        records_written = d.pop("records_written")

        stage = StageMetricsType4Stage(d.pop("stage"))

        stage_metrics_type_4 = cls(
            graph_edges_created=graph_edges_created,
            graph_nodes_created=graph_nodes_created,
            records_written=records_written,
            stage=stage,
        )

        stage_metrics_type_4.additional_properties = d
        return stage_metrics_type_4

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
