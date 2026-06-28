# Copyright (c) 2025 Varshith Gudur. Licensed under MIT OR Apache-2.0.
"""
ValoricoreAdapter — Drop-in wrapper for any existing vector DB.

Adds deterministic proof generation without changing existing
insert/search behavior. All crypto runs in Rust via the FFI.

Proofs are stored in Valoricore's KernelState as Record.metadata —
event-sourced, snapshot-persisted, included in global state hash.
"""

import numpy as np
from typing import Optional

# Import bridge functions from Rust FFI
from .valoricore_ffi import verify_embedding
from .local import LocalClient


class ValoricoreAdapter:
    """
    Wraps any existing vector DB + a Valoricore kernel for proof storage.

    - External DB handles storage and search (unchanged)
    - Valoricore kernel handles proofs (persisted, deterministic)

    Usage:
        from valoricore import ValoricoreAdapter
        from valoricore.local import LocalClient

        valoricore = LocalClient(path="./proof_store")
        db = ValoricoreAdapter(your_pinecone_client, valoricore=valoricore)
        proof = db.insert("doc_001", embedding)
    """

    def __init__(self, existing_db, valoricore: Optional[LocalClient] = None, proof_db_path: str = "./valoricore_proofs"):
        self.db = existing_db
        # Kernel-backed proof store
        self._valoricore = valoricore or LocalClient(path=proof_db_path)
        # Map external IDs → kernel record IDs (for retrieval)
        self._id_map: dict[str, int] = {}

    def insert(
        self,
        id: str,
        embedding: np.ndarray,
        metadata: dict = None
    ) -> str:
        """
        Insert into existing DB and generate a kernel-backed proof.

        Returns:
            Proof hash (hex string) — BLAKE3 Merkle root, persisted in kernel.
        """
        # 1. Store in existing DB — untouched
        self.db.insert(id, embedding, metadata or {})

        # 2. Single atomic Rust call — proof baked into Record.metadata
        record_id, proof_hash = self._valoricore.kernel.insert_with_proof(
            embedding.flatten().tolist(),
            tag=0
        )

        # 3. Track the mapping
        self._id_map[id] = record_id
        return proof_hash

    def insert_batch(
        self,
        ids: list[str],
        embeddings: list[np.ndarray],
        metadata_list: list[dict] = None
    ) -> list[str]:
        """
        Insert a batch into existing DB and generate kernel-backed proofs.

        Returns:
            List of proof hashes (hex strings).
        """
        if metadata_list is None:
            metadata_list = [{}] * len(ids)

        # 1. Store in existing DB
        if hasattr(self.db, "insert_batch"):
            self.db.insert_batch(ids, embeddings, metadata_list)
        else:
            # Fallback for DBs without batch method
            for i in range(len(ids)):
                self.db.insert(ids[i], embeddings[i], metadata_list[i])

        # 2. Single atomic Rust call for the batch
        vectors = [emb.flatten().tolist() for emb in embeddings]
        results = self._valoricore.insert_batch_with_proof(vectors)

        # 3. Track mapping and collect proofs
        proof_hashes = []
        for i, (record_id, proof_hash) in enumerate(results):
            self._id_map[ids[i]] = record_id
            proof_hashes.append(proof_hash)

        return proof_hashes

    def search(
        self,
        query_embedding: np.ndarray,
        k: int = 10
    ) -> list[dict]:
        """
        Search existing DB and attach verification status to results.
        """
        results = self.db.search(query_embedding, k)

        for result in results:
            record_id = self._id_map.get(result["id"])
            if record_id is not None and "embedding" in result:
                # Get proof from kernel metadata
                meta = self._valoricore.kernel.get_metadata(record_id)
                if meta is not None:
                    stored_hash = bytes(meta).hex()
                    result["verified"] = verify_embedding(
                        result["embedding"].flatten().tolist()
                        if isinstance(result["embedding"], np.ndarray)
                        else result["embedding"],
                        stored_hash
                    )
                    result["proof_hash"] = stored_hash
                else:
                    result["verified"] = None
            else:
                result["verified"] = None

        return results

    def get_proof(self, id: str) -> Optional[str]:
        """Get the stored proof hash for a given external ID."""
        record_id = self._id_map.get(id)
        if record_id is None:
            return None
        meta = self._valoricore.kernel.get_metadata(record_id)
        if meta is None:
            return None
        return bytes(meta).hex()

    def verify(self, id: str, embedding: np.ndarray) -> bool:
        """
        Verify an embedding against its kernel-stored proof.

        Returns False if no proof exists for the given ID.
        """
        proof_hash = self.get_proof(id)
        if proof_hash is None:
            return False
        return verify_embedding(embedding.flatten().tolist(), proof_hash)
