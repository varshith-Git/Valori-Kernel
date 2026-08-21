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
    from ..models.receipt_fragment import ReceiptFragment


T = TypeVar("T", bound="Receipt")


@_attrs_define
class Receipt:
    """The unified proof of one completed Operation, as it crosses the wire.

    Mirrors [`valori_effect::Receipt`], which lives in a crate with no `utoipa`
    dependency — the same translation-layer arrangement as [`ApiError`].

    # Why this type exists

    Phase API-3.3: `GET /v1/proof/receipt` and `GET /v1/proof/receipt/{id}`
    were annotated `body = Object`, which renders as a bare `type: object` with
    no properties. Generators produce `object` in TypeScript and
    `Dict[str, Any]` in Python — so the receipt, which is the entire point of
    a verifiable memory system, arrived in every SDK as an opaque blob with no
    discoverable field.

    The handlers return `serde_json::to_value(&Receipt)`, and `Receipt` is a
    fully concrete struct. Nothing about it was ever unknowable; it simply was
    not written down.

    `tests/api_contract.rs::receipt_dto_matches_the_runtime_receipt` serialises
    a real `Receipt` and diffs its key set against this type, so the two cannot
    drift.

        Attributes:
            cluster_mode (bool): Whether the producing node was running in cluster mode.
            committed_height (int): Committed log height at production time.
            embed_enabled (bool): Whether embedding was enabled on the node that produced this.
            fragments (list['ReceiptFragment']): Per-task fragments in topological order.
            graph_hash (str): `BLAKE3(op_hash ‖ fp.hash ‖ ctx_hash ‖ topo_order)` for the task graph.
            kernel_abi_version (int): Kernel ABI the operation ran against.
            operation_hash (str): `BLAKE3(kind ‖ inputs ‖ policy)` for the operation.
            parent_receipts (list[list[int]]): Parent receipt hashes in the Merkle DAG. Empty for a root receipt.
            planner_fingerprint_hash (str): `BLAKE3(version ‖ routing_config_hash ‖ feature_flags_hash ‖ schema_version)`.
            produced_at (int): Unix seconds. Deliberately excluded from `receipt_hash`.
            receipt_hash (list[int]): Content-addressed BLAKE3 of the receipt, as 32 raw bytes.

                `ReceiptHash` is a `[u8; 32]` newtype, so it crosses the wire as an
                array of 32 integers — not the hex string `to_hex()` produces.
            receipt_id (str): Unique id for this receipt.
            shard_count (int): Shard count on the producing node.
            shard_id (int): Shard that produced this receipt.
            state_hash_after (str): BLAKE3 hex of kernel state after. Equal to `before` for read-only operations.
            state_hash_before (str): BLAKE3 hex of kernel state before the operation.
    """

    cluster_mode: bool
    committed_height: int
    embed_enabled: bool
    fragments: list["ReceiptFragment"]
    graph_hash: str
    kernel_abi_version: int
    operation_hash: str
    parent_receipts: list[list[int]]
    planner_fingerprint_hash: str
    produced_at: int
    receipt_hash: list[int]
    receipt_id: str
    shard_count: int
    shard_id: int
    state_hash_after: str
    state_hash_before: str
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        cluster_mode = self.cluster_mode

        committed_height = self.committed_height

        embed_enabled = self.embed_enabled

        fragments = []
        for fragments_item_data in self.fragments:
            fragments_item = fragments_item_data.to_dict()
            fragments.append(fragments_item)

        graph_hash = self.graph_hash

        kernel_abi_version = self.kernel_abi_version

        operation_hash = self.operation_hash

        parent_receipts = []
        for parent_receipts_item_data in self.parent_receipts:
            parent_receipts_item = parent_receipts_item_data

            parent_receipts.append(parent_receipts_item)

        planner_fingerprint_hash = self.planner_fingerprint_hash

        produced_at = self.produced_at

        receipt_hash = self.receipt_hash

        receipt_id = self.receipt_id

        shard_count = self.shard_count

        shard_id = self.shard_id

        state_hash_after = self.state_hash_after

        state_hash_before = self.state_hash_before

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "cluster_mode": cluster_mode,
                "committed_height": committed_height,
                "embed_enabled": embed_enabled,
                "fragments": fragments,
                "graph_hash": graph_hash,
                "kernel_abi_version": kernel_abi_version,
                "operation_hash": operation_hash,
                "parent_receipts": parent_receipts,
                "planner_fingerprint_hash": planner_fingerprint_hash,
                "produced_at": produced_at,
                "receipt_hash": receipt_hash,
                "receipt_id": receipt_id,
                "shard_count": shard_count,
                "shard_id": shard_id,
                "state_hash_after": state_hash_after,
                "state_hash_before": state_hash_before,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        from ..models.receipt_fragment import ReceiptFragment

        d = dict(src_dict)
        cluster_mode = d.pop("cluster_mode")

        committed_height = d.pop("committed_height")

        embed_enabled = d.pop("embed_enabled")

        fragments = []
        _fragments = d.pop("fragments")
        for fragments_item_data in _fragments:
            fragments_item = ReceiptFragment.from_dict(fragments_item_data)

            fragments.append(fragments_item)

        graph_hash = d.pop("graph_hash")

        kernel_abi_version = d.pop("kernel_abi_version")

        operation_hash = d.pop("operation_hash")

        parent_receipts = []
        _parent_receipts = d.pop("parent_receipts")
        for parent_receipts_item_data in _parent_receipts:
            parent_receipts_item = cast(list[int], parent_receipts_item_data)

            parent_receipts.append(parent_receipts_item)

        planner_fingerprint_hash = d.pop("planner_fingerprint_hash")

        produced_at = d.pop("produced_at")

        receipt_hash = cast(list[int], d.pop("receipt_hash"))

        receipt_id = d.pop("receipt_id")

        shard_count = d.pop("shard_count")

        shard_id = d.pop("shard_id")

        state_hash_after = d.pop("state_hash_after")

        state_hash_before = d.pop("state_hash_before")

        receipt = cls(
            cluster_mode=cluster_mode,
            committed_height=committed_height,
            embed_enabled=embed_enabled,
            fragments=fragments,
            graph_hash=graph_hash,
            kernel_abi_version=kernel_abi_version,
            operation_hash=operation_hash,
            parent_receipts=parent_receipts,
            planner_fingerprint_hash=planner_fingerprint_hash,
            produced_at=produced_at,
            receipt_hash=receipt_hash,
            receipt_id=receipt_id,
            shard_count=shard_count,
            shard_id=shard_id,
            state_hash_after=state_hash_after,
            state_hash_before=state_hash_before,
        )

        receipt.additional_properties = d
        return receipt

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
