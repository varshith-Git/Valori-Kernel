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
    from ..models.tree_answer_result import TreeAnswerResult
    from ..models.tree_hybrid_hit import TreeHybridHit


T = TypeVar("T", bound="TreeHybridResponse")


@_attrs_define
class TreeHybridResponse:
    """
    Attributes:
        hits (list['TreeHybridHit']):
        query (str):
        reasoning (str):
        tree_hit_count (int):
        vector_hit_count (int):
        tree_answer (Union['TreeAnswerResult', None, Unset]):
    """

    hits: list["TreeHybridHit"]
    query: str
    reasoning: str
    tree_hit_count: int
    vector_hit_count: int
    tree_answer: Union["TreeAnswerResult", None, Unset] = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        from ..models.tree_answer_result import TreeAnswerResult

        hits = []
        for hits_item_data in self.hits:
            hits_item = hits_item_data.to_dict()
            hits.append(hits_item)

        query = self.query

        reasoning = self.reasoning

        tree_hit_count = self.tree_hit_count

        vector_hit_count = self.vector_hit_count

        tree_answer: Union[None, Unset, dict[str, Any]]
        if isinstance(self.tree_answer, Unset):
            tree_answer = UNSET
        elif isinstance(self.tree_answer, TreeAnswerResult):
            tree_answer = self.tree_answer.to_dict()
        else:
            tree_answer = self.tree_answer

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "hits": hits,
                "query": query,
                "reasoning": reasoning,
                "tree_hit_count": tree_hit_count,
                "vector_hit_count": vector_hit_count,
            }
        )
        if tree_answer is not UNSET:
            field_dict["tree_answer"] = tree_answer

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        from ..models.tree_answer_result import TreeAnswerResult
        from ..models.tree_hybrid_hit import TreeHybridHit

        d = dict(src_dict)
        hits = []
        _hits = d.pop("hits")
        for hits_item_data in _hits:
            hits_item = TreeHybridHit.from_dict(hits_item_data)

            hits.append(hits_item)

        query = d.pop("query")

        reasoning = d.pop("reasoning")

        tree_hit_count = d.pop("tree_hit_count")

        vector_hit_count = d.pop("vector_hit_count")

        def _parse_tree_answer(data: object) -> Union["TreeAnswerResult", None, Unset]:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            try:
                if not isinstance(data, dict):
                    raise TypeError()
                tree_answer_type_1 = TreeAnswerResult.from_dict(data)

                return tree_answer_type_1
            except:  # noqa: E722
                pass
            return cast(Union["TreeAnswerResult", None, Unset], data)

        tree_answer = _parse_tree_answer(d.pop("tree_answer", UNSET))

        tree_hybrid_response = cls(
            hits=hits,
            query=query,
            reasoning=reasoning,
            tree_hit_count=tree_hit_count,
            vector_hit_count=vector_hit_count,
            tree_answer=tree_answer,
        )

        tree_hybrid_response.additional_properties = d
        return tree_hybrid_response

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
