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


T = TypeVar("T", bound="TreeHybridRequest")


@_attrs_define
class TreeHybridRequest:
    """
    Attributes:
        query (str):
        cache_key (Union[None, Unset, str]):
        doc_name (Union[None, Unset, str]):
        k (Union[Unset, int]):
        namespace (Union[None, Unset, str]):
        prev_hash (Union[None, Unset, str]):
        text (Union[None, Unset, str]):
        tree (Union['TreeIndex', None, Unset]):
        tree_weight (Union[Unset, float]):
    """

    query: str
    cache_key: Union[None, Unset, str] = UNSET
    doc_name: Union[None, Unset, str] = UNSET
    k: Union[Unset, int] = UNSET
    namespace: Union[None, Unset, str] = UNSET
    prev_hash: Union[None, Unset, str] = UNSET
    text: Union[None, Unset, str] = UNSET
    tree: Union["TreeIndex", None, Unset] = UNSET
    tree_weight: Union[Unset, float] = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        from ..models.tree_index import TreeIndex

        query = self.query

        cache_key: Union[None, Unset, str]
        if isinstance(self.cache_key, Unset):
            cache_key = UNSET
        else:
            cache_key = self.cache_key

        doc_name: Union[None, Unset, str]
        if isinstance(self.doc_name, Unset):
            doc_name = UNSET
        else:
            doc_name = self.doc_name

        k = self.k

        namespace: Union[None, Unset, str]
        if isinstance(self.namespace, Unset):
            namespace = UNSET
        else:
            namespace = self.namespace

        prev_hash: Union[None, Unset, str]
        if isinstance(self.prev_hash, Unset):
            prev_hash = UNSET
        else:
            prev_hash = self.prev_hash

        text: Union[None, Unset, str]
        if isinstance(self.text, Unset):
            text = UNSET
        else:
            text = self.text

        tree: Union[None, Unset, dict[str, Any]]
        if isinstance(self.tree, Unset):
            tree = UNSET
        elif isinstance(self.tree, TreeIndex):
            tree = self.tree.to_dict()
        else:
            tree = self.tree

        tree_weight = self.tree_weight

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "query": query,
            }
        )
        if cache_key is not UNSET:
            field_dict["cache_key"] = cache_key
        if doc_name is not UNSET:
            field_dict["doc_name"] = doc_name
        if k is not UNSET:
            field_dict["k"] = k
        if namespace is not UNSET:
            field_dict["namespace"] = namespace
        if prev_hash is not UNSET:
            field_dict["prev_hash"] = prev_hash
        if text is not UNSET:
            field_dict["text"] = text
        if tree is not UNSET:
            field_dict["tree"] = tree
        if tree_weight is not UNSET:
            field_dict["tree_weight"] = tree_weight

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

        def _parse_doc_name(data: object) -> Union[None, Unset, str]:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(Union[None, Unset, str], data)

        doc_name = _parse_doc_name(d.pop("doc_name", UNSET))

        k = d.pop("k", UNSET)

        def _parse_namespace(data: object) -> Union[None, Unset, str]:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(Union[None, Unset, str], data)

        namespace = _parse_namespace(d.pop("namespace", UNSET))

        def _parse_prev_hash(data: object) -> Union[None, Unset, str]:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(Union[None, Unset, str], data)

        prev_hash = _parse_prev_hash(d.pop("prev_hash", UNSET))

        def _parse_text(data: object) -> Union[None, Unset, str]:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(Union[None, Unset, str], data)

        text = _parse_text(d.pop("text", UNSET))

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

        tree_weight = d.pop("tree_weight", UNSET)

        tree_hybrid_request = cls(
            query=query,
            cache_key=cache_key,
            doc_name=doc_name,
            k=k,
            namespace=namespace,
            prev_hash=prev_hash,
            text=text,
            tree=tree,
            tree_weight=tree_weight,
        )

        tree_hybrid_request.additional_properties = d
        return tree_hybrid_request

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
