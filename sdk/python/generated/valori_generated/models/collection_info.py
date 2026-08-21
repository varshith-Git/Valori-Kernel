from collections.abc import Mapping
from typing import (
    Any,
    TypeVar,
    Union,
    cast,
)

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..models.index_kind import IndexKind
from ..models.metric import Metric
from ..types import UNSET, Unset

T = TypeVar("T", bound="CollectionInfo")


@_attrs_define
class CollectionInfo:
    """
    Attributes:
        id (int):
        name (str):
        dimension (Union[None, Unset, int]): Present only for collections created with an explicit vector config.
        index (Union[IndexKind, None, Unset]):
        max_records (Union[None, Unset, int]):
        metric (Union[Metric, None, Unset]):
        record_count (Union[None, Unset, int]):
    """

    id: int
    name: str
    dimension: Union[None, Unset, int] = UNSET
    index: Union[IndexKind, None, Unset] = UNSET
    max_records: Union[None, Unset, int] = UNSET
    metric: Union[Metric, None, Unset] = UNSET
    record_count: Union[None, Unset, int] = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        id = self.id

        name = self.name

        dimension: Union[None, Unset, int]
        if isinstance(self.dimension, Unset):
            dimension = UNSET
        else:
            dimension = self.dimension

        index: Union[None, Unset, str]
        if isinstance(self.index, Unset):
            index = UNSET
        elif isinstance(self.index, IndexKind):
            index = self.index.value
        else:
            index = self.index

        max_records: Union[None, Unset, int]
        if isinstance(self.max_records, Unset):
            max_records = UNSET
        else:
            max_records = self.max_records

        metric: Union[None, Unset, str]
        if isinstance(self.metric, Unset):
            metric = UNSET
        elif isinstance(self.metric, Metric):
            metric = self.metric.value
        else:
            metric = self.metric

        record_count: Union[None, Unset, int]
        if isinstance(self.record_count, Unset):
            record_count = UNSET
        else:
            record_count = self.record_count

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "id": id,
                "name": name,
            }
        )
        if dimension is not UNSET:
            field_dict["dimension"] = dimension
        if index is not UNSET:
            field_dict["index"] = index
        if max_records is not UNSET:
            field_dict["max_records"] = max_records
        if metric is not UNSET:
            field_dict["metric"] = metric
        if record_count is not UNSET:
            field_dict["record_count"] = record_count

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)
        id = d.pop("id")

        name = d.pop("name")

        def _parse_dimension(data: object) -> Union[None, Unset, int]:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(Union[None, Unset, int], data)

        dimension = _parse_dimension(d.pop("dimension", UNSET))

        def _parse_index(data: object) -> Union[IndexKind, None, Unset]:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            try:
                if not isinstance(data, str):
                    raise TypeError()
                index_type_1 = IndexKind(data)

                return index_type_1
            except:  # noqa: E722
                pass
            return cast(Union[IndexKind, None, Unset], data)

        index = _parse_index(d.pop("index", UNSET))

        def _parse_max_records(data: object) -> Union[None, Unset, int]:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(Union[None, Unset, int], data)

        max_records = _parse_max_records(d.pop("max_records", UNSET))

        def _parse_metric(data: object) -> Union[Metric, None, Unset]:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            try:
                if not isinstance(data, str):
                    raise TypeError()
                metric_type_1 = Metric(data)

                return metric_type_1
            except:  # noqa: E722
                pass
            return cast(Union[Metric, None, Unset], data)

        metric = _parse_metric(d.pop("metric", UNSET))

        def _parse_record_count(data: object) -> Union[None, Unset, int]:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(Union[None, Unset, int], data)

        record_count = _parse_record_count(d.pop("record_count", UNSET))

        collection_info = cls(
            id=id,
            name=name,
            dimension=dimension,
            index=index,
            max_records=max_records,
            metric=metric,
            record_count=record_count,
        )

        collection_info.additional_properties = d
        return collection_info

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
