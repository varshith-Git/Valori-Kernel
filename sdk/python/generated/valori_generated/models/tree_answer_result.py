from collections.abc import Mapping
from typing import (
    TYPE_CHECKING,
    Any,
    TypeVar,
    cast,
)

from attrs import define as _attrs_define
from attrs import field as _attrs_field

if TYPE_CHECKING:
    from ..models.tree_citation import TreeCitation
    from ..models.tree_receipt import TreeReceipt


T = TypeVar("T", bound="TreeAnswerResult")


@_attrs_define
class TreeAnswerResult:
    """
    Attributes:
        answer (str):
        citations (list['TreeCitation']):
        evidence_text (str):
        fetched_ranges (list[list[int]]):
        query (str):
        reasoning (str):
        receipt (TreeReceipt): One tamper-evident record of a single retrieval, chained with BLAKE3.
        visited_node_ids (list[str]):
    """

    answer: str
    citations: list["TreeCitation"]
    evidence_text: str
    fetched_ranges: list[list[int]]
    query: str
    reasoning: str
    receipt: "TreeReceipt"
    visited_node_ids: list[str]
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        answer = self.answer

        citations = []
        for citations_item_data in self.citations:
            citations_item = citations_item_data.to_dict()
            citations.append(citations_item)

        evidence_text = self.evidence_text

        fetched_ranges = []
        for fetched_ranges_item_data in self.fetched_ranges:
            fetched_ranges_item = fetched_ranges_item_data

            fetched_ranges.append(fetched_ranges_item)

        query = self.query

        reasoning = self.reasoning

        receipt = self.receipt.to_dict()

        visited_node_ids = self.visited_node_ids

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "answer": answer,
                "citations": citations,
                "evidence_text": evidence_text,
                "fetched_ranges": fetched_ranges,
                "query": query,
                "reasoning": reasoning,
                "receipt": receipt,
                "visited_node_ids": visited_node_ids,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        from ..models.tree_citation import TreeCitation
        from ..models.tree_receipt import TreeReceipt

        d = dict(src_dict)
        answer = d.pop("answer")

        citations = []
        _citations = d.pop("citations")
        for citations_item_data in _citations:
            citations_item = TreeCitation.from_dict(citations_item_data)

            citations.append(citations_item)

        evidence_text = d.pop("evidence_text")

        fetched_ranges = []
        _fetched_ranges = d.pop("fetched_ranges")
        for fetched_ranges_item_data in _fetched_ranges:
            fetched_ranges_item = cast(list[int], fetched_ranges_item_data)

            fetched_ranges.append(fetched_ranges_item)

        query = d.pop("query")

        reasoning = d.pop("reasoning")

        receipt = TreeReceipt.from_dict(d.pop("receipt"))

        visited_node_ids = cast(list[str], d.pop("visited_node_ids"))

        tree_answer_result = cls(
            answer=answer,
            citations=citations,
            evidence_text=evidence_text,
            fetched_ranges=fetched_ranges,
            query=query,
            reasoning=reasoning,
            receipt=receipt,
            visited_node_ids=visited_node_ids,
        )

        tree_answer_result.additional_properties = d
        return tree_answer_result

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
