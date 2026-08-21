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
    from ..models.multi_search_hit import MultiSearchHit
    from ..models.partial_search_failure import PartialSearchFailure


T = TypeVar("T", bound="MultiSearchResponse")


@_attrs_define
class MultiSearchResponse:
    """Response for `POST /v1/search/multi`.

    Attributes:
        collections_searched (list[str]): Names of all collections included in this query.
        results (list['MultiSearchHit']): Global top-k hits sorted by score ascending (smaller = closer).
        partial_failures (Union[None, Unset, list['PartialSearchFailure']]): Runtime failures from individual
            collections, if any.
            Present only when at least one collection's search failed after
            dimension/metric compatibility was confirmed.
    """

    collections_searched: list[str]
    results: list["MultiSearchHit"]
    partial_failures: Union[None, Unset, list["PartialSearchFailure"]] = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        collections_searched = self.collections_searched

        results = []
        for results_item_data in self.results:
            results_item = results_item_data.to_dict()
            results.append(results_item)

        partial_failures: Union[None, Unset, list[dict[str, Any]]]
        if isinstance(self.partial_failures, Unset):
            partial_failures = UNSET
        elif isinstance(self.partial_failures, list):
            partial_failures = []
            for partial_failures_type_0_item_data in self.partial_failures:
                partial_failures_type_0_item = (
                    partial_failures_type_0_item_data.to_dict()
                )
                partial_failures.append(partial_failures_type_0_item)

        else:
            partial_failures = self.partial_failures

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "collections_searched": collections_searched,
                "results": results,
            }
        )
        if partial_failures is not UNSET:
            field_dict["partial_failures"] = partial_failures

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        from ..models.multi_search_hit import MultiSearchHit
        from ..models.partial_search_failure import PartialSearchFailure

        d = dict(src_dict)
        collections_searched = cast(list[str], d.pop("collections_searched"))

        results = []
        _results = d.pop("results")
        for results_item_data in _results:
            results_item = MultiSearchHit.from_dict(results_item_data)

            results.append(results_item)

        def _parse_partial_failures(
            data: object,
        ) -> Union[None, Unset, list["PartialSearchFailure"]]:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            try:
                if not isinstance(data, list):
                    raise TypeError()
                partial_failures_type_0 = []
                _partial_failures_type_0 = data
                for partial_failures_type_0_item_data in _partial_failures_type_0:
                    partial_failures_type_0_item = PartialSearchFailure.from_dict(
                        partial_failures_type_0_item_data
                    )

                    partial_failures_type_0.append(partial_failures_type_0_item)

                return partial_failures_type_0
            except:  # noqa: E722
                pass
            return cast(Union[None, Unset, list["PartialSearchFailure"]], data)

        partial_failures = _parse_partial_failures(d.pop("partial_failures", UNSET))

        multi_search_response = cls(
            collections_searched=collections_searched,
            results=results,
            partial_failures=partial_failures,
        )

        multi_search_response.additional_properties = d
        return multi_search_response

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
