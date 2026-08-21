# Copyright (c) 2025 Varshith Gudur. Licensed under MIT OR Apache-2.0.
"""Node-scoped resources: meta, ingest, tree, community, proof, snapshots,
storage, cluster and crypto.

These operations are not scoped to a collection (or take the collection inside
their body), so they hang off :class:`~valori.client.ValoriClient` directly
rather than off a :class:`~valori.resources.collections.Collection`.
"""

from __future__ import annotations

from typing import Any, BinaryIO, Mapping, Optional, Sequence

from valori_generated.api.cluster import (
    get_cluster_health as _get_cluster_health,
    get_cluster_proof as _get_cluster_proof,
    get_cluster_role as _get_cluster_role,
    get_cluster_status as _get_cluster_status,
)
from valori_generated.api.community import (
    community_detect as _community_detect,
    community_overview as _community_overview,
    community_search as _community_search,
    extract_entities as _extract_entities,
)
from valori_generated.api.crypto import get_key_status as _get_key_status
from valori_generated.api.ingest import (
    chunk_document as _chunk_document,
    get_ingest_status as _get_ingest_status,
    ingest_document as _ingest_document,
    update_ingested_document as _update_ingested_document,
)
from valori_generated.api.meta import (
    get_health as _get_health,
    get_models_health as _get_models_health,
    get_shard_routing as _get_shard_routing,
    get_usage as _get_usage,
    get_version as _get_version,
)
from valori_generated.api.proof import (
    get_event_log_proof as _get_event_log_proof,
    get_latest_receipt as _get_latest_receipt,
    get_receipt as _get_receipt,
    get_state_proof as _get_state_proof,
    get_timeline as _get_timeline,
)
from valori_generated.api.snapshot import (
    download_snapshot as _download_snapshot,
    restore_snapshot as _restore_snapshot,
    save_snapshot as _save_snapshot,
    upload_snapshot as _upload_snapshot,
)
from valori_generated.api.storage import (
    archive_wal_segment as _archive_wal_segment,
    get_storage_manifest as _get_storage_manifest,
    list_archived_wal_segments as _list_archived_wal_segments,
    list_object_store_snapshots as _list_object_store_snapshots,
    restore_snapshot_from_object_store as _restore_snapshot_from_object_store,
    upload_snapshot_to_object_store as _upload_snapshot_to_object_store,
)
from valori_generated.api.tree import (
    tree_build as _tree_build,
    tree_chain_verify as _tree_chain_verify,
    tree_hybrid as _tree_hybrid,
    tree_query as _tree_query,
    tree_verify as _tree_verify,
)
from valori_generated.models.archive_wal_request import ArchiveWalRequest
from valori_generated.models.community_detect_request import CommunityDetectRequest
from valori_generated.models.community_search_request import CommunitySearchRequest
from valori_generated.models.extract_entities_request import ExtractEntitiesRequest
from valori_generated.models.ingest_document_request import IngestDocumentRequest
from valori_generated.models.ingest_request import IngestRequest
from valori_generated.models.ingest_update_request import IngestUpdateRequest
from valori_generated.models.restore_from_store_request import RestoreFromStoreRequest
from valori_generated.models.snapshot_restore_request import SnapshotRestoreRequest
from valori_generated.models.snapshot_save_request import SnapshotSaveRequest
from valori_generated.models.tree_build_request import TreeBuildRequest
from valori_generated.models.tree_chain_verify_request import TreeChainVerifyRequest
from valori_generated.models.tree_hybrid_request import TreeHybridRequest
from valori_generated.models.tree_query_request import TreeQueryRequest
from valori_generated.models.tree_verify_request import TreeVerifyRequest
from valori_generated.types import File

from .._models import build
from ..transport import unset_if_none
from ._base import Resource

__all__ = [
    "Meta",
    "Ingest",
    "Tree",
    "Community",
    "Proof",
    "Snapshots",
    "Storage",
    "Cluster",
    "Crypto",
]


class Meta(Resource):
    """``client.meta`` — health, version, usage and topology."""

    def health(self) -> Any:
        """``GET /health``. The one unauthenticated operation in the contract."""
        return self._t.call(_get_health)

    def version(self) -> Any:
        """``GET /v1/version``."""
        return self._t.call(_get_version)

    def usage(self) -> Any:
        """``GET /v1/usage``."""
        return self._t.call(_get_usage)

    def models_health(self) -> Any:
        """``GET /v1/models/health``."""
        return self._t.call(_get_models_health)

    def shard_routing(self) -> Any:
        """``GET /v1/shard/routing``."""
        return self._t.call(_get_shard_routing)


class Ingest(Resource):
    """``client.ingest`` — the server-side chunk + embed + insert pipeline."""

    def chunk(
        self,
        text: str,
        *,
        collection: Optional[str] = None,
        source: Optional[str] = None,
        strategy: Optional[str] = None,
        chunk_size: Optional[int] = None,
        chunk_overlap: Optional[int] = None,
    ) -> Any:
        """Chunk without embedding or storing. ``POST /v1/ingest/document``."""
        body = build(
            IngestDocumentRequest,
            text=text,
            collection=collection,
            source=source,
            strategy=strategy,
            chunk_size=chunk_size,
            chunk_overlap=chunk_overlap,
        )
        return self._t.call(_chunk_document, body=body)

    def document(
        self,
        text: str,
        *,
        collection: Optional[str] = None,
        source: Optional[str] = None,
        strategy: Optional[str] = None,
        chunk_size: Optional[int] = None,
        chunk_overlap: Optional[int] = None,
        background: Optional[bool] = None,
    ) -> Any:
        """Full ingest. ``POST /v1/ingest``.

        ``background=True`` maps to the ``async`` query flag and makes the node
        answer 202 with a job id — poll it with :meth:`status`.
        """
        body = build(
            IngestRequest,
            text=text,
            collection=collection,
            source=source,
            strategy=strategy,
            chunk_size=chunk_size,
            chunk_overlap=chunk_overlap,
        )
        return self._t.call(_ingest_document, body=body, async_=unset_if_none(background))

    def update(
        self,
        document_node_id: int,
        text: str,
        *,
        collection: Optional[str] = None,
        source: Optional[str] = None,
        strategy: Optional[str] = None,
        chunk_size: Optional[int] = None,
        chunk_overlap: Optional[int] = None,
    ) -> Any:
        """Diff-based document update. ``POST /v1/ingest/update``."""
        body = build(
            IngestUpdateRequest,
            document_node_id=document_node_id,
            text=text,
            collection=collection,
            source=source,
            strategy=strategy,
            chunk_size=chunk_size,
            chunk_overlap=chunk_overlap,
        )
        return self._t.call(_update_ingested_document, body=body)

    def status(self, job_id: str) -> Any:
        """``GET /v1/ingest/status/{job_id}``."""
        return self._t.call(_get_ingest_status, job_id=job_id)

    def extract_entities(
        self,
        text: str,
        *,
        namespace: Optional[str] = None,
        entity_types: Optional[Sequence[str]] = None,
        model: Optional[str] = None,
    ) -> Any:
        """LLM entity extraction. ``POST /v1/ingest/extract-entities``."""
        body = build(
            ExtractEntitiesRequest,
            text=text,
            namespace=namespace,
            entity_types=list(entity_types) if entity_types is not None else None,
            model=model,
        )
        return self._t.call(_extract_entities, body=body)


class Tree(Resource):
    """``client.tree`` — Tree-RAG build, query and receipt verification."""

    def build(self, text: str, *, doc_name: Optional[str] = None) -> Any:
        """``POST /v1/tree/build``. Returns a ``cache_key`` to reuse."""
        return self._t.call(_tree_build, body=build(TreeBuildRequest, text=text, doc_name=doc_name))

    def query(
        self,
        query: str,
        *,
        tree: Optional[Any] = None,
        cache_key: Optional[str] = None,
        k: Optional[int] = None,
        prev_hash: Optional[str] = None,
    ) -> Any:
        """``POST /v1/tree/query``."""
        body = build(
            TreeQueryRequest, query=query, tree=tree, cache_key=cache_key, k=k, prev_hash=prev_hash
        )
        return self._t.call(_tree_query, body=body)

    def hybrid(
        self,
        query: str,
        *,
        text: Optional[str] = None,
        tree: Optional[Any] = None,
        cache_key: Optional[str] = None,
        namespace: Optional[str] = None,
        k: Optional[int] = None,
        tree_weight: Optional[float] = None,
        prev_hash: Optional[str] = None,
        doc_name: Optional[str] = None,
    ) -> Any:
        """``POST /v1/tree/hybrid``."""
        body = build(
            TreeHybridRequest,
            query=query,
            text=text,
            tree=tree,
            cache_key=cache_key,
            namespace=namespace,
            k=k,
            tree_weight=tree_weight,
            prev_hash=prev_hash,
            doc_name=doc_name,
        )
        return self._t.call(_tree_hybrid, body=body)

    def verify(self, tree: Any, receipt: Any) -> Any:
        """``POST /v1/tree/verify``. Stateless."""
        return self._t.call(_tree_verify, body=build(TreeVerifyRequest, tree=tree, receipt=receipt))

    def chain_verify(self, receipts: Sequence[Any]) -> Any:
        """``POST /v1/tree/chain-verify``. Verifies a whole receipt chain."""
        return self._t.call(
            _tree_chain_verify, body=build(TreeChainVerifyRequest, receipts=list(receipts))
        )


class Community(Resource):
    """``client.community`` — label-propagation communities over the graph."""

    def detect(self, *, namespace: Optional[str] = None, max_iter: Optional[int] = None) -> Any:
        """``POST /v1/community/detect``. Run before search or overview."""
        body = build(CommunityDetectRequest, namespace=namespace, max_iter=max_iter)
        return self._t.call(_community_detect, body=body)

    def search(
        self,
        vector: Sequence[float],
        *,
        k: Optional[int] = None,
        namespace: Optional[str] = None,
        depth: Optional[int] = None,
        drill_in: Optional[bool] = None,
    ) -> Any:
        """``POST /v1/community/search``."""
        body = build(
            CommunitySearchRequest,
            vector=list(vector),
            k=k,
            namespace=namespace,
            depth=depth,
            drill_in=drill_in,
        )
        return self._t.call(_community_search, body=body)

    def overview(self) -> Any:
        """``GET /v1/community/overview``."""
        return self._t.call(_community_overview)


class Proof(Resource):
    """``client.proof`` — the verifiability surface."""

    def event_log(self) -> Any:
        """``GET /v1/proof/event-log`` — the receipt primitive."""
        return self._t.call(_get_event_log_proof)

    def state(self) -> Any:
        """``GET /v1/proof/state``."""
        return self._t.call(_get_state_proof)

    def receipt(self, receipt_id: str) -> Any:
        """``GET /v1/proof/receipt/{id}``."""
        return self._t.call(_get_receipt, id=receipt_id)

    def latest_receipt(self) -> Any:
        """``GET /v1/proof/receipt``."""
        return self._t.call(_get_latest_receipt)

    def timeline(
        self,
        *,
        from_: Optional[str] = None,
        to: Optional[str] = None,
        limit: Optional[int] = None,
        collection: Optional[str] = None,
    ) -> Any:
        """``GET /v1/timeline``."""
        return self._t.call(
            _get_timeline,
            from_=unset_if_none(from_),
            to=unset_if_none(to),
            limit=unset_if_none(limit),
            collection=unset_if_none(collection),
        )


class Snapshots(Resource):
    """``client.snapshots`` — local snapshot save/restore/transfer."""

    def save(self, *, path: Optional[str] = None) -> Any:
        """``POST /v1/snapshot/save``."""
        return self._t.call(_save_snapshot, body=build(SnapshotSaveRequest, path=path))

    def restore(self, path: str) -> Any:
        """``POST /v1/snapshot/restore``."""
        return self._t.call(_restore_snapshot, body=build(SnapshotRestoreRequest, path=path))

    def download(self) -> Any:
        """``GET /v1/snapshot/download``."""
        return self._t.call(_download_snapshot)

    def upload(self, payload: BinaryIO, *, file_name: Optional[str] = None) -> Any:
        """``POST /v1/snapshot/upload``. ``payload`` is an open binary file."""
        return self._t.call(
            _upload_snapshot,
            body=File(payload=payload, file_name=file_name, mime_type="application/octet-stream"),
        )


class Storage(Resource):
    """``client.storage`` — object-store offload and WAL archival."""

    def upload_snapshot(self) -> Any:
        """``POST /v1/storage/snapshots/upload``."""
        return self._t.call(_upload_snapshot_to_object_store)

    def restore_snapshot(self, *, key: Optional[str] = None) -> Any:
        """``POST /v1/storage/snapshots/restore``. Omit ``key`` to use the manifest."""
        return self._t.call(
            _restore_snapshot_from_object_store, body=build(RestoreFromStoreRequest, key=key)
        )

    def list_snapshots(self) -> Any:
        """``GET /v1/storage/snapshots``."""
        return self._t.call(_list_object_store_snapshots)

    def manifest(self) -> Any:
        """``GET /v1/storage/manifest`` — the disaster-recovery entry point."""
        return self._t.call(_get_storage_manifest)

    def archive_wal(self, path: str) -> Any:
        """``POST /v1/storage/wal/archive``."""
        return self._t.call(_archive_wal_segment, body=build(ArchiveWalRequest, path=path))

    def list_wal_segments(self) -> Any:
        """``GET /v1/storage/wal``."""
        return self._t.call(_list_archived_wal_segments)


class Cluster(Resource):
    """``client.cluster`` — the cluster-mode read surface."""

    def status(self) -> Any:
        """``GET /v1/cluster/status``."""
        return self._t.call(_get_cluster_status)

    def health(self) -> Any:
        """``GET /v1/cluster/health``."""
        return self._t.call(_get_cluster_health)

    def role(self) -> Any:
        """``GET /v1/cluster/role``."""
        return self._t.call(_get_cluster_role)

    def proof(self) -> Any:
        """``GET /v1/cluster/proof`` — the cluster analog of ``proof.state``."""
        return self._t.call(_get_cluster_proof)


class Crypto(Resource):
    """``client.crypto`` — per-key crypto-shredding status."""

    def key_status(self, key_id: str) -> Any:
        """``GET /v1/crypto/status/{key_id}``."""
        return self._t.call(_get_key_status, key_id=key_id)
