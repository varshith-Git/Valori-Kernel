from collections.abc import Mapping
from typing import Any, TypeVar

from attrs import define as _attrs_define
from attrs import field as _attrs_field

T = TypeVar("T", bound="ReceiptFragment")


@_attrs_define
class ReceiptFragment:
    """One task's contribution to a [`ReceiptDto`], mirroring
    [`valori_effect::ReceiptFragment`].

        Attributes:
            fragment_hash (str): BLAKE3 hex of the fragment itself, used for chaining.
            mutated (bool): True if this task produced kernel writes.
            state_hash_after (str): BLAKE3 hex of the kernel state after this task. Equal to `before` for reads.
            state_hash_before (str): BLAKE3 hex of the kernel state before this task.
            task_index (int): Position of this task in the executed graph's topological order.
    """

    fragment_hash: str
    mutated: bool
    state_hash_after: str
    state_hash_before: str
    task_index: int
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        fragment_hash = self.fragment_hash

        mutated = self.mutated

        state_hash_after = self.state_hash_after

        state_hash_before = self.state_hash_before

        task_index = self.task_index

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "fragment_hash": fragment_hash,
                "mutated": mutated,
                "state_hash_after": state_hash_after,
                "state_hash_before": state_hash_before,
                "task_index": task_index,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)
        fragment_hash = d.pop("fragment_hash")

        mutated = d.pop("mutated")

        state_hash_after = d.pop("state_hash_after")

        state_hash_before = d.pop("state_hash_before")

        task_index = d.pop("task_index")

        receipt_fragment = cls(
            fragment_hash=fragment_hash,
            mutated=mutated,
            state_hash_after=state_hash_after,
            state_hash_before=state_hash_before,
            task_index=task_index,
        )

        receipt_fragment.additional_properties = d
        return receipt_fragment

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
