from collections.abc import Mapping
from typing import Any, TypeVar

from attrs import define as _attrs_define
from attrs import field as _attrs_field

T = TypeVar("T", bound="PoolStatsSchema")


@_attrs_define
class PoolStatsSchema:
    """Slab occupancy for one kernel pool (records, graph nodes, graph edges).

    Attributes:
        capacity (int): Configured slab capacity.
        fill_pct (float): `live / capacity` as a percentage, rounded to one decimal.
        live (int): Live (non-tombstoned) entries.
        slots_used (int): Slab slots consumed, including tombstones.
    """

    capacity: int
    fill_pct: float
    live: int
    slots_used: int
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        capacity = self.capacity

        fill_pct = self.fill_pct

        live = self.live

        slots_used = self.slots_used

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "capacity": capacity,
                "fill_pct": fill_pct,
                "live": live,
                "slots_used": slots_used,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)
        capacity = d.pop("capacity")

        fill_pct = d.pop("fill_pct")

        live = d.pop("live")

        slots_used = d.pop("slots_used")

        pool_stats_schema = cls(
            capacity=capacity,
            fill_pct=fill_pct,
            live=live,
            slots_used=slots_used,
        )

        pool_stats_schema.additional_properties = d
        return pool_stats_schema

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
