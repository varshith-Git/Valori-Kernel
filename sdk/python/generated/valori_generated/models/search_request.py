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
    from ..models.graph_rerank_request import GraphRerankRequest
    from ..models.search_request_metadata_filter_type_0 import (
        SearchRequestMetadataFilterType0,
    )


T = TypeVar("T", bound="SearchRequest")


@_attrs_define
class SearchRequest:
    """
    Attributes:
        k (int):
        query (list[float]):
        as_of (Union[None, Unset, str]): ISO 8601 UTC timestamp — search the vector state as it existed at this moment.
            Requires the event log to be enabled (`VALORI_EVENT_LOG_PATH`).
        as_of_log_index (Union[None, Unset, int]): Log index — search the vector state after exactly this many committed
            events.
            Mutually exclusive with `as_of`; `as_of_log_index` takes precedence if both given.
        collection (Union[None, Unset, str]):
        decay_half_life_secs (Union[None, Unset, int]): Phase C4.1 — recency half-life in seconds. When set (> 0),
            results are
            re-ranked so older records decay: a record one half-life old has its L2
            distance doubled. `0`/absent uses the server default (or pure distance).
            Ignored for `as_of` / point-in-time queries.
        graph_rerank (Union['GraphRerankRequest', None, Unset]):
        metadata_filter (Union['SearchRequestMetadataFilterType0', None, Unset]): Optional JSON object whose key-value
            pairs must ALL be present (and equal)
            in a record's metadata for the record to be returned.
            Numeric values support optional range operators: `{"gte": 2020, "lte": 2024}`.
            Example: `{"author": "Alice", "year": {"gte": 2020}}`
        query_text (Union[None, Unset, str]): The raw query string used for BM25 scoring. Required when `rerank=true`.
            Ignored when `rerank=false`.
        rerank (Union[Unset, bool]): BM25 hybrid reranking. When `true` (default), the server fetches
            `k × POOL_FACTOR` candidates by vector similarity and re-ranks them by
            a 50/50 blend of normalised vector score + BM25 term-frequency score
            before returning the top-k. Requires `query_text` to be set.
            Set to `false` to get pure vector ranking (legacy behaviour).
    """

    k: int
    query: list[float]
    as_of: Union[None, Unset, str] = UNSET
    as_of_log_index: Union[None, Unset, int] = UNSET
    collection: Union[None, Unset, str] = UNSET
    decay_half_life_secs: Union[None, Unset, int] = UNSET
    graph_rerank: Union["GraphRerankRequest", None, Unset] = UNSET
    metadata_filter: Union["SearchRequestMetadataFilterType0", None, Unset] = UNSET
    query_text: Union[None, Unset, str] = UNSET
    rerank: Union[Unset, bool] = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        from ..models.graph_rerank_request import GraphRerankRequest
        from ..models.search_request_metadata_filter_type_0 import (
            SearchRequestMetadataFilterType0,
        )

        k = self.k

        query = self.query

        as_of: Union[None, Unset, str]
        if isinstance(self.as_of, Unset):
            as_of = UNSET
        else:
            as_of = self.as_of

        as_of_log_index: Union[None, Unset, int]
        if isinstance(self.as_of_log_index, Unset):
            as_of_log_index = UNSET
        else:
            as_of_log_index = self.as_of_log_index

        collection: Union[None, Unset, str]
        if isinstance(self.collection, Unset):
            collection = UNSET
        else:
            collection = self.collection

        decay_half_life_secs: Union[None, Unset, int]
        if isinstance(self.decay_half_life_secs, Unset):
            decay_half_life_secs = UNSET
        else:
            decay_half_life_secs = self.decay_half_life_secs

        graph_rerank: Union[None, Unset, dict[str, Any]]
        if isinstance(self.graph_rerank, Unset):
            graph_rerank = UNSET
        elif isinstance(self.graph_rerank, GraphRerankRequest):
            graph_rerank = self.graph_rerank.to_dict()
        else:
            graph_rerank = self.graph_rerank

        metadata_filter: Union[None, Unset, dict[str, Any]]
        if isinstance(self.metadata_filter, Unset):
            metadata_filter = UNSET
        elif isinstance(self.metadata_filter, SearchRequestMetadataFilterType0):
            metadata_filter = self.metadata_filter.to_dict()
        else:
            metadata_filter = self.metadata_filter

        query_text: Union[None, Unset, str]
        if isinstance(self.query_text, Unset):
            query_text = UNSET
        else:
            query_text = self.query_text

        rerank = self.rerank

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "k": k,
                "query": query,
            }
        )
        if as_of is not UNSET:
            field_dict["as_of"] = as_of
        if as_of_log_index is not UNSET:
            field_dict["as_of_log_index"] = as_of_log_index
        if collection is not UNSET:
            field_dict["collection"] = collection
        if decay_half_life_secs is not UNSET:
            field_dict["decay_half_life_secs"] = decay_half_life_secs
        if graph_rerank is not UNSET:
            field_dict["graph_rerank"] = graph_rerank
        if metadata_filter is not UNSET:
            field_dict["metadata_filter"] = metadata_filter
        if query_text is not UNSET:
            field_dict["query_text"] = query_text
        if rerank is not UNSET:
            field_dict["rerank"] = rerank

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        from ..models.graph_rerank_request import GraphRerankRequest
        from ..models.search_request_metadata_filter_type_0 import (
            SearchRequestMetadataFilterType0,
        )

        d = dict(src_dict)
        k = d.pop("k")

        query = cast(list[float], d.pop("query"))

        def _parse_as_of(data: object) -> Union[None, Unset, str]:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(Union[None, Unset, str], data)

        as_of = _parse_as_of(d.pop("as_of", UNSET))

        def _parse_as_of_log_index(data: object) -> Union[None, Unset, int]:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(Union[None, Unset, int], data)

        as_of_log_index = _parse_as_of_log_index(d.pop("as_of_log_index", UNSET))

        def _parse_collection(data: object) -> Union[None, Unset, str]:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(Union[None, Unset, str], data)

        collection = _parse_collection(d.pop("collection", UNSET))

        def _parse_decay_half_life_secs(data: object) -> Union[None, Unset, int]:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(Union[None, Unset, int], data)

        decay_half_life_secs = _parse_decay_half_life_secs(
            d.pop("decay_half_life_secs", UNSET)
        )

        def _parse_graph_rerank(
            data: object,
        ) -> Union["GraphRerankRequest", None, Unset]:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            try:
                if not isinstance(data, dict):
                    raise TypeError()
                graph_rerank_type_1 = GraphRerankRequest.from_dict(data)

                return graph_rerank_type_1
            except:  # noqa: E722
                pass
            return cast(Union["GraphRerankRequest", None, Unset], data)

        graph_rerank = _parse_graph_rerank(d.pop("graph_rerank", UNSET))

        def _parse_metadata_filter(
            data: object,
        ) -> Union["SearchRequestMetadataFilterType0", None, Unset]:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            try:
                if not isinstance(data, dict):
                    raise TypeError()
                metadata_filter_type_0 = SearchRequestMetadataFilterType0.from_dict(
                    data
                )

                return metadata_filter_type_0
            except:  # noqa: E722
                pass
            return cast(Union["SearchRequestMetadataFilterType0", None, Unset], data)

        metadata_filter = _parse_metadata_filter(d.pop("metadata_filter", UNSET))

        def _parse_query_text(data: object) -> Union[None, Unset, str]:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(Union[None, Unset, str], data)

        query_text = _parse_query_text(d.pop("query_text", UNSET))

        rerank = d.pop("rerank", UNSET)

        search_request = cls(
            k=k,
            query=query,
            as_of=as_of,
            as_of_log_index=as_of_log_index,
            collection=collection,
            decay_half_life_secs=decay_half_life_secs,
            graph_rerank=graph_rerank,
            metadata_filter=metadata_filter,
            query_text=query_text,
            rerank=rerank,
        )

        search_request.additional_properties = d
        return search_request

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
