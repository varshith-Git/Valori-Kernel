from collections.abc import Mapping
from typing import (
    Any,
    TypeVar,
    Union,
    cast,
)

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..models.index_kind_input import IndexKindInput
from ..models.metric_input import MetricInput
from ..types import UNSET, Unset

T = TypeVar("T", bound="CreateCollectionRequest")


@_attrs_define
class CreateCollectionRequest:
    """
    Attributes:
        dimension (Union[None, int]): Vector dimension for this collection. **Required for every name** —
            `"default"` included, with no exception (Phase 3.3). Valori does not
            infer a collection's dimension from its first insert or from any
            project/env-level default; `POST` without it is rejected with 400.
            Immutable after creation; a later request with a different value for
            the same collection is rejected, not silently applied.

            The Rust field stays `Option` so that a missing value reaches
            `parse_collection_config` and is answered with that explanatory 400
            rather than a generic deserialization failure. `required = true`
            records the contract-level truth the handler enforces, so a generated
            SDK makes the argument mandatory instead of silently omittable.
        metric (Union[MetricInput, None]):
        name (str):
        index (Union[IndexKindInput, None, Unset]):
    """

    dimension: Union[None, int]
    metric: Union[MetricInput, None]
    name: str
    index: Union[IndexKindInput, None, Unset] = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        dimension: Union[None, int]
        dimension = self.dimension

        metric: Union[None, str]
        if isinstance(self.metric, MetricInput):
            metric = self.metric.value
        else:
            metric = self.metric

        name = self.name

        index: Union[None, Unset, str]
        if isinstance(self.index, Unset):
            index = UNSET
        elif isinstance(self.index, IndexKindInput):
            index = self.index.value
        else:
            index = self.index

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "dimension": dimension,
                "metric": metric,
                "name": name,
            }
        )
        if index is not UNSET:
            field_dict["index"] = index

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)

        def _parse_dimension(data: object) -> Union[None, int]:
            if data is None:
                return data
            return cast(Union[None, int], data)

        dimension = _parse_dimension(d.pop("dimension"))

        def _parse_metric(data: object) -> Union[MetricInput, None]:
            if data is None:
                return data
            try:
                if not isinstance(data, str):
                    raise TypeError()
                metric_type_1 = MetricInput(data)

                return metric_type_1
            except:  # noqa: E722
                pass
            return cast(Union[MetricInput, None], data)

        metric = _parse_metric(d.pop("metric"))

        name = d.pop("name")

        def _parse_index(data: object) -> Union[IndexKindInput, None, Unset]:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            try:
                if not isinstance(data, str):
                    raise TypeError()
                index_type_1 = IndexKindInput(data)

                return index_type_1
            except:  # noqa: E722
                pass
            return cast(Union[IndexKindInput, None, Unset], data)

        index = _parse_index(d.pop("index", UNSET))

        create_collection_request = cls(
            dimension=dimension,
            metric=metric,
            name=name,
            index=index,
        )

        create_collection_request.additional_properties = d
        return create_collection_request

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
