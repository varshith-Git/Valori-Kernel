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
    from ..models.tree_index import TreeIndex


T = TypeVar("T", bound="TreeQueryRequest")


@_attrs_define
class TreeQueryRequest:
    """
    Attributes:
        query (str):
        cache_key (Union[None, Unset, str]):
        k (Union[Unset, int]):
        prev_hash (Union[None, Unset, str]):
        tree (Union['TreeIndex', None, Unset]):
    """

    query: str
    cache_key: Union[None, Unset, str] = UNSET
    k: Union[Unset, int] = UNSET
    prev_hash: Union[None, Unset, str] = UNSET
    tree: Union["TreeIndex", None, Unset] = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        from ..models.tree_index import TreeIndex

        query = self.query

        cache_key: Union[None, Unset, str]
        if isinstance(self.cache_key, Unset):
            cache_key = UNSET
        else:
            cache_key = self.cache_key

        k = self.k

        prev_hash: Union[None, Unset, str]
        if isinstance(self.prev_hash, Unset):
            prev_hash = UNSET
        else:
            prev_hash = self.prev_hash

        tree: Union[None, Unset, dict[str, Any]]
        if isinstance(self.tree, Unset):
            tree = UNSET
        elif isinstance(self.tree, TreeIndex):
            tree = self.tree.to_dict()
        else:
            tree = self.tree

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "query": query,
            }
        )
        if cache_key is not UNSET:
            field_dict["cache_key"] = cache_key
        if k is not UNSET:
            field_dict["k"] = k
        if prev_hash is not UNSET:
            field_dict["prev_hash"] = prev_hash
        if tree is not UNSET:
            field_dict["tree"] = tree

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        from ..models.tree_index import TreeIndex

        d = dict(src_dict)
        query = d.pop("query")

        def _parse_cache_key(data: object) -> Union[None, Unset, str]:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(Union[None, Unset, str], data)

        cache_key = _parse_cache_key(d.pop("cache_key", UNSET))

        k = d.pop("k", UNSET)

        def _parse_prev_hash(data: object) -> Union[None, Unset, str]:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(Union[None, Unset, str], data)

        prev_hash = _parse_prev_hash(d.pop("prev_hash", UNSET))

        def _parse_tree(data: object) -> Union["TreeIndex", None, Unset]:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            try:
                if not isinstance(data, dict):
                    raise TypeError()
                tree_type_1 = TreeIndex.from_dict(data)

                return tree_type_1
            except:  # noqa: E722
                pass
            return cast(Union["TreeIndex", None, Unset], data)

        tree = _parse_tree(d.pop("tree", UNSET))

        tree_query_request = cls(
            query=query,
            cache_key=cache_key,
            k=k,
            prev_hash=prev_hash,
            tree=tree,
        )

        tree_query_request.additional_properties = d
        return tree_query_request

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
