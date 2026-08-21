from collections.abc import Mapping
from typing import (
    Any,
    TypeVar,
    Union,
    cast,
)

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

T = TypeVar("T", bound="OperationOverview")


@_attrs_define
class OperationOverview:
    """The `overview` block of [`OperationDetailResponse`].

    Attributes:
        collection (str):
        id (str):
        status (str):
        timing (str):
        type_ (str):
        edge_id (Union[None, Unset, int]): Set when the operation touched a graph edge.
        log_index (Union[None, Unset, int]): Position in the committed event log.
        node_id (Union[None, Unset, int]): Set when the operation touched a graph node.
        record_id (Union[None, Unset, int]): Set when the operation touched a record.
    """

    collection: str
    id: str
    status: str
    timing: str
    type_: str
    edge_id: Union[None, Unset, int] = UNSET
    log_index: Union[None, Unset, int] = UNSET
    node_id: Union[None, Unset, int] = UNSET
    record_id: Union[None, Unset, int] = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        collection = self.collection

        id = self.id

        status = self.status

        timing = self.timing

        type_ = self.type_

        edge_id: Union[None, Unset, int]
        if isinstance(self.edge_id, Unset):
            edge_id = UNSET
        else:
            edge_id = self.edge_id

        log_index: Union[None, Unset, int]
        if isinstance(self.log_index, Unset):
            log_index = UNSET
        else:
            log_index = self.log_index

        node_id: Union[None, Unset, int]
        if isinstance(self.node_id, Unset):
            node_id = UNSET
        else:
            node_id = self.node_id

        record_id: Union[None, Unset, int]
        if isinstance(self.record_id, Unset):
            record_id = UNSET
        else:
            record_id = self.record_id

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "collection": collection,
                "id": id,
                "status": status,
                "timing": timing,
                "type": type_,
            }
        )
        if edge_id is not UNSET:
            field_dict["edge_id"] = edge_id
        if log_index is not UNSET:
            field_dict["log_index"] = log_index
        if node_id is not UNSET:
            field_dict["node_id"] = node_id
        if record_id is not UNSET:
            field_dict["record_id"] = record_id

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)
        collection = d.pop("collection")

        id = d.pop("id")

        status = d.pop("status")

        timing = d.pop("timing")

        type_ = d.pop("type")

        def _parse_edge_id(data: object) -> Union[None, Unset, int]:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(Union[None, Unset, int], data)

        edge_id = _parse_edge_id(d.pop("edge_id", UNSET))

        def _parse_log_index(data: object) -> Union[None, Unset, int]:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(Union[None, Unset, int], data)

        log_index = _parse_log_index(d.pop("log_index", UNSET))

        def _parse_node_id(data: object) -> Union[None, Unset, int]:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(Union[None, Unset, int], data)

        node_id = _parse_node_id(d.pop("node_id", UNSET))

        def _parse_record_id(data: object) -> Union[None, Unset, int]:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(Union[None, Unset, int], data)

        record_id = _parse_record_id(d.pop("record_id", UNSET))

        operation_overview = cls(
            collection=collection,
            id=id,
            status=status,
            timing=timing,
            type_=type_,
            edge_id=edge_id,
            log_index=log_index,
            node_id=node_id,
            record_id=record_id,
        )

        operation_overview.additional_properties = d
        return operation_overview

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
