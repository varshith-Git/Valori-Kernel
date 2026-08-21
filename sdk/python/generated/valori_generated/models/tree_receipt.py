from collections.abc import Mapping
from typing import (
    Any,
    TypeVar,
    cast,
)

from attrs import define as _attrs_define
from attrs import field as _attrs_field

T = TypeVar("T", bound="TreeReceipt")


@_attrs_define
class TreeReceipt:
    """One tamper-evident record of a single retrieval, chained with BLAKE3.

    Attributes:
        answer_hash (str):
        evidence_hash (str):
        fetched_ranges (list[list[int]]):
        hash_algo (str):
        prev_hash (str):
        query (str):
        query_hash (str):
        receipt_hash (str):
        timestamp (int):
        visited_node_ids (list[str]):
    """

    answer_hash: str
    evidence_hash: str
    fetched_ranges: list[list[int]]
    hash_algo: str
    prev_hash: str
    query: str
    query_hash: str
    receipt_hash: str
    timestamp: int
    visited_node_ids: list[str]
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        answer_hash = self.answer_hash

        evidence_hash = self.evidence_hash

        fetched_ranges = []
        for fetched_ranges_item_data in self.fetched_ranges:
            fetched_ranges_item = fetched_ranges_item_data

            fetched_ranges.append(fetched_ranges_item)

        hash_algo = self.hash_algo

        prev_hash = self.prev_hash

        query = self.query

        query_hash = self.query_hash

        receipt_hash = self.receipt_hash

        timestamp = self.timestamp

        visited_node_ids = self.visited_node_ids

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "answer_hash": answer_hash,
                "evidence_hash": evidence_hash,
                "fetched_ranges": fetched_ranges,
                "hash_algo": hash_algo,
                "prev_hash": prev_hash,
                "query": query,
                "query_hash": query_hash,
                "receipt_hash": receipt_hash,
                "timestamp": timestamp,
                "visited_node_ids": visited_node_ids,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)
        answer_hash = d.pop("answer_hash")

        evidence_hash = d.pop("evidence_hash")

        fetched_ranges = []
        _fetched_ranges = d.pop("fetched_ranges")
        for fetched_ranges_item_data in _fetched_ranges:
            fetched_ranges_item = cast(list[int], fetched_ranges_item_data)

            fetched_ranges.append(fetched_ranges_item)

        hash_algo = d.pop("hash_algo")

        prev_hash = d.pop("prev_hash")

        query = d.pop("query")

        query_hash = d.pop("query_hash")

        receipt_hash = d.pop("receipt_hash")

        timestamp = d.pop("timestamp")

        visited_node_ids = cast(list[str], d.pop("visited_node_ids"))

        tree_receipt = cls(
            answer_hash=answer_hash,
            evidence_hash=evidence_hash,
            fetched_ranges=fetched_ranges,
            hash_algo=hash_algo,
            prev_hash=prev_hash,
            query=query,
            query_hash=query_hash,
            receipt_hash=receipt_hash,
            timestamp=timestamp,
            visited_node_ids=visited_node_ids,
        )

        tree_receipt.additional_properties = d
        return tree_receipt

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
