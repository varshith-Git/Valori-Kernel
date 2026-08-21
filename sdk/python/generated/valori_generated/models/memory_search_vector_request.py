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
    from ..models.memory_search_vector_request_metadata_filter_type_0 import (
        MemorySearchVectorRequestMetadataFilterType0,
    )


T = TypeVar("T", bound="MemorySearchVectorRequest")


@_attrs_define
class MemorySearchVectorRequest:
    """
    Attributes:
        k (int):
        query_vector (list[float]):
        collection (Union[None, Unset, str]):
        consistency (Union[None, Unset, str]): Phase S6 (cluster mode only; ignored standalone): `"local"` skips
            the read-index round trip (eventually consistent, faster). Absent
            or any other value defaults to linearizable, matching `/v1/search`.
        decay_half_life_secs (Union[None, Unset, int]): Phase C4.1 — recency half-life (seconds). When set (> 0), the
            agent-memory
            recall path re-ranks older memories down. See `SearchRequest`.
        metadata_filter (Union['MemorySearchVectorRequestMetadataFilterType0', None, Unset]): Phase I7 — restrict
            results to records whose stored metadata satisfies
            every key/value predicate. Same semantics as `SearchRequest::metadata_filter`.
        query_text (Union[None, Unset, str]): Phase C5 — raw query text for BM25 hybrid re-ranking. Required when
            `rerank=true`; ignored otherwise.
        rerank (Union[Unset, bool]): Phase C5 — when `true` (default) and `query_text` is provided, re-ranks
            candidates by hybrid BM25 + vector score before returning the top-k.
    """

    k: int
    query_vector: list[float]
    collection: Union[None, Unset, str] = UNSET
    consistency: Union[None, Unset, str] = UNSET
    decay_half_life_secs: Union[None, Unset, int] = UNSET
    metadata_filter: Union[
        "MemorySearchVectorRequestMetadataFilterType0", None, Unset
    ] = UNSET
    query_text: Union[None, Unset, str] = UNSET
    rerank: Union[Unset, bool] = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        from ..models.memory_search_vector_request_metadata_filter_type_0 import (
            MemorySearchVectorRequestMetadataFilterType0,
        )

        k = self.k

        query_vector = self.query_vector

        collection: Union[None, Unset, str]
        if isinstance(self.collection, Unset):
            collection = UNSET
        else:
            collection = self.collection

        consistency: Union[None, Unset, str]
        if isinstance(self.consistency, Unset):
            consistency = UNSET
        else:
            consistency = self.consistency

        decay_half_life_secs: Union[None, Unset, int]
        if isinstance(self.decay_half_life_secs, Unset):
            decay_half_life_secs = UNSET
        else:
            decay_half_life_secs = self.decay_half_life_secs

        metadata_filter: Union[None, Unset, dict[str, Any]]
        if isinstance(self.metadata_filter, Unset):
            metadata_filter = UNSET
        elif isinstance(
            self.metadata_filter, MemorySearchVectorRequestMetadataFilterType0
        ):
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
                "query_vector": query_vector,
            }
        )
        if collection is not UNSET:
            field_dict["collection"] = collection
        if consistency is not UNSET:
            field_dict["consistency"] = consistency
        if decay_half_life_secs is not UNSET:
            field_dict["decay_half_life_secs"] = decay_half_life_secs
        if metadata_filter is not UNSET:
            field_dict["metadata_filter"] = metadata_filter
        if query_text is not UNSET:
            field_dict["query_text"] = query_text
        if rerank is not UNSET:
            field_dict["rerank"] = rerank

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        from ..models.memory_search_vector_request_metadata_filter_type_0 import (
            MemorySearchVectorRequestMetadataFilterType0,
        )

        d = dict(src_dict)
        k = d.pop("k")

        query_vector = cast(list[float], d.pop("query_vector"))

        def _parse_collection(data: object) -> Union[None, Unset, str]:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(Union[None, Unset, str], data)

        collection = _parse_collection(d.pop("collection", UNSET))

        def _parse_consistency(data: object) -> Union[None, Unset, str]:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(Union[None, Unset, str], data)

        consistency = _parse_consistency(d.pop("consistency", UNSET))

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
        ) -> Union["MemorySearchVectorRequestMetadataFilterType0", None, Unset]:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            try:
                if not isinstance(data, dict):
                    raise TypeError()
                metadata_filter_type_0 = (
                    MemorySearchVectorRequestMetadataFilterType0.from_dict(data)
                )

                return metadata_filter_type_0
            except:  # noqa: E722
                pass
            return cast(
                Union["MemorySearchVectorRequestMetadataFilterType0", None, Unset], data
            )

        metadata_filter = _parse_metadata_filter(d.pop("metadata_filter", UNSET))

        def _parse_query_text(data: object) -> Union[None, Unset, str]:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(Union[None, Unset, str], data)

        query_text = _parse_query_text(d.pop("query_text", UNSET))

        rerank = d.pop("rerank", UNSET)

        memory_search_vector_request = cls(
            k=k,
            query_vector=query_vector,
            collection=collection,
            consistency=consistency,
            decay_half_life_secs=decay_half_life_secs,
            metadata_filter=metadata_filter,
            query_text=query_text,
            rerank=rerank,
        )

        memory_search_vector_request.additional_properties = d
        return memory_search_vector_request

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
