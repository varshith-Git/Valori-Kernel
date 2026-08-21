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

from ..types import UNSET, Unset

if TYPE_CHECKING:
    from ..models.timeline_entry import TimelineEntry


T = TypeVar("T", bound="TimelineResponse")


@_attrs_define
class TimelineResponse:
    """
    Attributes:
        events (list['TimelineEntry']):
        total (int):
        from_unix (Union[None, Unset, int]): Inclusive lower bound filter applied (unix seconds), if any.
        to_unix (Union[None, Unset, int]): Inclusive upper bound filter applied (unix seconds), if any.
    """

    events: list["TimelineEntry"]
    total: int
    from_unix: Union[None, Unset, int] = UNSET
    to_unix: Union[None, Unset, int] = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        events = []
        for events_item_data in self.events:
            events_item = events_item_data.to_dict()
            events.append(events_item)

        total = self.total

        from_unix: Union[None, Unset, int]
        if isinstance(self.from_unix, Unset):
            from_unix = UNSET
        else:
            from_unix = self.from_unix

        to_unix: Union[None, Unset, int]
        if isinstance(self.to_unix, Unset):
            to_unix = UNSET
        else:
            to_unix = self.to_unix

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "events": events,
                "total": total,
            }
        )
        if from_unix is not UNSET:
            field_dict["from_unix"] = from_unix
        if to_unix is not UNSET:
            field_dict["to_unix"] = to_unix

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        from ..models.timeline_entry import TimelineEntry

        d = dict(src_dict)
        events = []
        _events = d.pop("events")
        for events_item_data in _events:
            events_item = TimelineEntry.from_dict(events_item_data)

            events.append(events_item)

        total = d.pop("total")

        def _parse_from_unix(data: object) -> Union[None, Unset, int]:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(Union[None, Unset, int], data)

        from_unix = _parse_from_unix(d.pop("from_unix", UNSET))

        def _parse_to_unix(data: object) -> Union[None, Unset, int]:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(Union[None, Unset, int], data)

        to_unix = _parse_to_unix(d.pop("to_unix", UNSET))

        timeline_response = cls(
            events=events,
            total=total,
            from_unix=from_unix,
            to_unix=to_unix,
        )

        timeline_response.additional_properties = d
        return timeline_response

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
