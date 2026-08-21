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
    from ..models.multi_search_request_metadata_filter_type_0 import (
        MultiSearchRequestMetadataFilterType0,
    )


T = TypeVar("T", bound="MultiSearchRequest")


@_attrs_define
class MultiSearchRequest:
    """Request body for `POST /v1/search/multi`.

    Attributes:
        collections (list[str]): One or more collection names. All must share the same `dim` and `metric`.
        k (int): Number of global top-k results to return.
        query (list[float]): Query vector. Must match the shared dimension of all listed collections.
        decay_half_life_secs (Union[None, Unset, int]): Phase C4.1 — decay half-life in seconds. Applied per-collection
            before merge.
        metadata_filter (Union['MultiSearchRequestMetadataFilterType0', None, Unset]): Metadata predicate applied per-
            collection after vector search.
    """

    collections: list[str]
    k: int
    query: list[float]
    decay_half_life_secs: Union[None, Unset, int] = UNSET
    metadata_filter: Union["MultiSearchRequestMetadataFilterType0", None, Unset] = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        from ..models.multi_search_request_metadata_filter_type_0 import (
            MultiSearchRequestMetadataFilterType0,
        )

        collections = self.collections

        k = self.k

        query = self.query

        decay_half_life_secs: Union[None, Unset, int]
        if isinstance(self.decay_half_life_secs, Unset):
            decay_half_life_secs = UNSET
        else:
            decay_half_life_secs = self.decay_half_life_secs

        metadata_filter: Union[None, Unset, dict[str, Any]]
        if isinstance(self.metadata_filter, Unset):
            metadata_filter = UNSET
        elif isinstance(self.metadata_filter, MultiSearchRequestMetadataFilterType0):
            metadata_filter = self.metadata_filter.to_dict()
        else:
            metadata_filter = self.metadata_filter

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "collections": collections,
                "k": k,
                "query": query,
            }
        )
        if decay_half_life_secs is not UNSET:
            field_dict["decay_half_life_secs"] = decay_half_life_secs
        if metadata_filter is not UNSET:
            field_dict["metadata_filter"] = metadata_filter

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        from ..models.multi_search_request_metadata_filter_type_0 import (
            MultiSearchRequestMetadataFilterType0,
        )

        d = dict(src_dict)
        collections = cast(list[str], d.pop("collections"))

        k = d.pop("k")

        query = cast(list[float], d.pop("query"))

        def _parse_decay_half_life_secs(data: object) -> Union[None, Unset, int]:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(Union[None, Unset, int], data)

        decay_half_life_secs = _parse_decay_half_life_secs(
            d.pop("decay_half_life_secs", UNSET)
        )

        def _parse_metadata_filter(
            data: object,
        ) -> Union["MultiSearchRequestMetadataFilterType0", None, Unset]:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            try:
                if not isinstance(data, dict):
                    raise TypeError()
                metadata_filter_type_0 = (
                    MultiSearchRequestMetadataFilterType0.from_dict(data)
                )

                return metadata_filter_type_0
            except:  # noqa: E722
                pass
            return cast(
                Union["MultiSearchRequestMetadataFilterType0", None, Unset], data
            )

        metadata_filter = _parse_metadata_filter(d.pop("metadata_filter", UNSET))

        multi_search_request = cls(
            collections=collections,
            k=k,
            query=query,
            decay_half_life_secs=decay_half_life_secs,
            metadata_filter=metadata_filter,
        )

        multi_search_request.additional_properties = d
        return multi_search_request

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
