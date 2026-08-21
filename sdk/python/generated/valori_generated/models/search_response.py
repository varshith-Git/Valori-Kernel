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
    from ..models.search_hit import SearchHit


T = TypeVar("T", bound="SearchResponse")


@_attrs_define
class SearchResponse:
    """
    Attributes:
        results (list['SearchHit']):
        as_of_log_index (Union[None, Unset, int]): Present only for as-of searches: the log index of the replayed state.
        as_of_state_hash (Union[None, Unset, str]): BLAKE3 hex hash of the kernel state at `as_of_log_index`.
        as_of_timestamp_iso (Union[None, Unset, str]): ISO 8601 string of `as_of_timestamp_unix`.
        as_of_timestamp_unix (Union[None, Unset, int]): Unix-second wall-clock timestamp of the `as_of_log_index` event.
    """

    results: list["SearchHit"]
    as_of_log_index: Union[None, Unset, int] = UNSET
    as_of_state_hash: Union[None, Unset, str] = UNSET
    as_of_timestamp_iso: Union[None, Unset, str] = UNSET
    as_of_timestamp_unix: Union[None, Unset, int] = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        results = []
        for results_item_data in self.results:
            results_item = results_item_data.to_dict()
            results.append(results_item)

        as_of_log_index: Union[None, Unset, int]
        if isinstance(self.as_of_log_index, Unset):
            as_of_log_index = UNSET
        else:
            as_of_log_index = self.as_of_log_index

        as_of_state_hash: Union[None, Unset, str]
        if isinstance(self.as_of_state_hash, Unset):
            as_of_state_hash = UNSET
        else:
            as_of_state_hash = self.as_of_state_hash

        as_of_timestamp_iso: Union[None, Unset, str]
        if isinstance(self.as_of_timestamp_iso, Unset):
            as_of_timestamp_iso = UNSET
        else:
            as_of_timestamp_iso = self.as_of_timestamp_iso

        as_of_timestamp_unix: Union[None, Unset, int]
        if isinstance(self.as_of_timestamp_unix, Unset):
            as_of_timestamp_unix = UNSET
        else:
            as_of_timestamp_unix = self.as_of_timestamp_unix

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "results": results,
            }
        )
        if as_of_log_index is not UNSET:
            field_dict["as_of_log_index"] = as_of_log_index
        if as_of_state_hash is not UNSET:
            field_dict["as_of_state_hash"] = as_of_state_hash
        if as_of_timestamp_iso is not UNSET:
            field_dict["as_of_timestamp_iso"] = as_of_timestamp_iso
        if as_of_timestamp_unix is not UNSET:
            field_dict["as_of_timestamp_unix"] = as_of_timestamp_unix

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        from ..models.search_hit import SearchHit

        d = dict(src_dict)
        results = []
        _results = d.pop("results")
        for results_item_data in _results:
            results_item = SearchHit.from_dict(results_item_data)

            results.append(results_item)

        def _parse_as_of_log_index(data: object) -> Union[None, Unset, int]:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(Union[None, Unset, int], data)

        as_of_log_index = _parse_as_of_log_index(d.pop("as_of_log_index", UNSET))

        def _parse_as_of_state_hash(data: object) -> Union[None, Unset, str]:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(Union[None, Unset, str], data)

        as_of_state_hash = _parse_as_of_state_hash(d.pop("as_of_state_hash", UNSET))

        def _parse_as_of_timestamp_iso(data: object) -> Union[None, Unset, str]:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(Union[None, Unset, str], data)

        as_of_timestamp_iso = _parse_as_of_timestamp_iso(
            d.pop("as_of_timestamp_iso", UNSET)
        )

        def _parse_as_of_timestamp_unix(data: object) -> Union[None, Unset, int]:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(Union[None, Unset, int], data)

        as_of_timestamp_unix = _parse_as_of_timestamp_unix(
            d.pop("as_of_timestamp_unix", UNSET)
        )

        search_response = cls(
            results=results,
            as_of_log_index=as_of_log_index,
            as_of_state_hash=as_of_state_hash,
            as_of_timestamp_iso=as_of_timestamp_iso,
            as_of_timestamp_unix=as_of_timestamp_unix,
        )

        search_response.additional_properties = d
        return search_response

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
